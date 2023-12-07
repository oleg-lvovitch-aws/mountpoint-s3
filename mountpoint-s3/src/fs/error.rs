//! Utilities for handling errors generated by the `fs` module and mapping them to FUSE errors

use crate::inode::InodeError;
use crate::upload::UploadWriteError;

/// Generate an error that includes a conversion to a libc errno for use in replies to FUSE.
///
/// `mountpoint-s3` is an application, so we'd be happy to just use the [anyhow] crate directly,
/// except that we need to be able to convert every error into a C integer for use as an errno to
/// give FUSE the right reply. This macro builds a little wrapper around an [anyhow::Error] that
/// includes an errno. We also want to preserve the source information for errors whenever possible,
/// so we optionally allow providing a `source:` argument that will chain an error together with
/// this new one.
///
/// # Examples
///
/// If you already have an error, provide it as the source so that printed versions of the error
/// include its source. For example:
///
/// ```ignore
/// let err = client.head_object("DOC-EXAMPLE-BUCKET", "mykey").await.expect_err("failed");
/// return Err(err!(libc::ENOENT, source:err, "file does not exist"));
/// ```
/// will print "file does not exist: service error: ...".
///
///
/// Otherwise, build a new error with no source:
///
/// ```ignore
/// return Err(err!(libc::EINVAL, "cannot use O_SYNC on file handle {:?}", fh));
/// ```
#[macro_export]
macro_rules! err {
    // Base case -- don't use directly
    ($errno:expr, __source:$source:expr, $message:literal, $($args:tt)*) => {
        Error {
            errno: $errno,
            message: format!($message, $($args)*),
            source: $source,
        }
    };
    // Actual cases
    ($errno:expr, source:$source:expr, $message:literal, $($args:tt)*) => {
        err!($errno, __source:Some(::anyhow::Error::new($source)), $message, $($args)*)
    };
    ($errno:expr, source:$source:expr, $message:literal) => {
        err!($errno, __source:Some(::anyhow::Error::new($source)), $message,)
    };
    ($errno:expr, $message:literal, $($args:tt)*) => {
        err!($errno, __source:None, $message, $($args)*)
    };
    ($errno:expr, $message:literal) => {
        err!($errno, __source:None, $message,)
    };
}

/// A dynamic error type returned by the Mountpoint filesystem. See the [err!] macro for more
/// details.
#[derive(Debug, thiserror::Error)]
pub struct Error {
    pub(crate) errno: libc::c_int,
    pub(crate) message: String,
    pub(crate) source: Option<anyhow::Error>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(source) = self.source.as_ref() {
            // {:#} tells anyhow to include the entire chain of sources for the error
            write!(f, "{}: {:#}", self.message, source)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl From<InodeError> for Error {
    fn from(err: InodeError) -> Self {
        let errno = err.to_errno();
        Error {
            errno,
            message: String::from("inode error"),
            source: Some(anyhow::anyhow!(err)),
        }
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<UploadWriteError<E>> for Error {
    fn from(err: UploadWriteError<E>) -> Self {
        let errno = err.to_errno();
        Error {
            errno,
            message: String::from("upload error"),
            source: Some(anyhow::anyhow!(err)),
        }
    }
}

/// Errors that can be converted to a raw OS error (errno)
pub trait ToErrno {
    fn to_errno(&self) -> libc::c_int;
}

impl ToErrno for Error {
    fn to_errno(&self) -> libc::c_int {
        self.errno
    }
}

impl ToErrno for InodeError {
    fn to_errno(&self) -> libc::c_int {
        match self {
            InodeError::ClientError(_) => libc::EIO,
            InodeError::FileDoesNotExist(_, _) => libc::ENOENT,
            InodeError::InodeDoesNotExist(_) => libc::ENOENT,
            InodeError::InvalidFileName(_) => libc::EINVAL,
            InodeError::NotADirectory(_) => libc::ENOTDIR,
            InodeError::IsDirectory(_) => libc::EISDIR,
            InodeError::FileAlreadyExists(_) => libc::EEXIST,
            // Not obvious what InodeNotWritable, InodeAlreadyWriting, InodeNotReadableWhileWriting should be.
            // EINVAL or EROFS would also be reasonable -- but we'll treat them like sealed files.
            InodeError::InodeNotWritable(_) => libc::EPERM,
            InodeError::InodeAlreadyWriting(_) => libc::EPERM,
            InodeError::InodeNotReadableWhileWriting(_) => libc::EPERM,
            InodeError::CannotRemoveRemoteDirectory(_) => libc::EPERM,
            InodeError::DirectoryNotEmpty(_) => libc::ENOTEMPTY,
            InodeError::UnlinkNotPermittedWhileWriting(_) => libc::EPERM,
            InodeError::CorruptedMetadata(_) => libc::EIO,
            InodeError::SetAttrNotPermittedOnRemoteInode(_) => libc::EPERM,
            InodeError::StaleInode { .. } => libc::ESTALE,
        }
    }
}

impl<E: std::error::Error> ToErrno for UploadWriteError<E> {
    fn to_errno(&self) -> libc::c_int {
        match self {
            UploadWriteError::PutRequestFailed(_) => libc::EIO,
            UploadWriteError::OutOfOrderWrite { .. } => libc::EINVAL,
            UploadWriteError::ObjectTooBig { .. } => libc::EFBIG,
        }
    }
}
