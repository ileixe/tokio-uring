use crate::buf::fixed::FixedBuf;
use crate::WithBuffer;
use crate::{buf::BoundedBuf, io::SharedFd, OneshotOutputTransform, Result, UnsubmittedOneshot};
use io_uring::cqueue::Entry;
use std::io;
use std::marker::PhantomData;

/// An unsubmitted write operation.
pub type UnsubmittedWrite<T> = UnsubmittedOneshot<WriteData<T>, WriteTransform<T>>;

#[allow(missing_docs)]
pub struct WriteData<T> {
    /// Holds a strong ref to the FD, preventing the file from being closed
    /// while the operation is in-flight.
    _fd: SharedFd,

    buf: T,
}

#[allow(missing_docs)]
pub struct WriteTransform<T> {
    _phantom: PhantomData<T>,
}

impl<T> OneshotOutputTransform for WriteTransform<T> {
    type Output = Result<usize, T>;
    type StoredData = WriteData<T>;

    fn transform_oneshot_output(self, data: Self::StoredData, cqe: Entry) -> Self::Output {
        let res = if cqe.result() >= 0 {
            Ok(cqe.result() as usize)
        } else {
            Err(io::Error::from_raw_os_error(-cqe.result()))
        };

        res.with_buffer(data.buf)
    }
}

impl<T: BoundedBuf> UnsubmittedWrite<T> {
    pub(crate) fn write_at(fd: &SharedFd, buf: T, offset: u64) -> Self {
        use io_uring::{opcode, types};

        // Get raw buffer info
        let ptr = buf.stable_ptr();
        let len = buf.bytes_init();

        Self::new(
            WriteData {
                _fd: fd.clone(),
                buf,
            },
            WriteTransform {
                _phantom: PhantomData,
            },
            opcode::Write::new(types::Fd(fd.raw_fd()), ptr, len as _)
                .offset(offset as _)
                .build(),
        )
    }
}

impl<T: BoundedBuf<Buf = FixedBuf>> UnsubmittedWrite<T> {
    pub(crate) fn write_fixed_at(fd: &SharedFd, buf: T, offset: u64) -> Self {
        use io_uring::{opcode, types};

        // Get raw buffer info
        let ptr = buf.stable_ptr();
        let len = buf.bytes_init();
        let buf_index = buf.get_buf().buf_index();

        Self::new(
            WriteData {
                _fd: fd.clone(),
                buf,
            },
            WriteTransform {
                _phantom: PhantomData,
            },
            opcode::WriteFixed::new(types::Fd(fd.raw_fd()), ptr, len as _, buf_index)
                .offset(offset as _)
                .build(),
        )
    }
}

impl<T: BoundedBuf> UnsubmittedWrite<T> {
    pub(crate) fn write_fixed_at_with_index(
        fd: &SharedFd,
        buf: T,
        buf_index: u16,
        offset: u64,
    ) -> Self {
        use io_uring::{opcode, types};

        // Get raw buffer info
        let ptr = buf.stable_ptr();
        let len = buf.bytes_init();

        Self::new(
            WriteData {
                _fd: fd.clone(),
                buf,
            },
            WriteTransform {
                _phantom: PhantomData,
            },
            opcode::WriteFixed::new(types::Fd(fd.raw_fd()), ptr, len as _, buf_index)
                .offset(offset as _)
                .build(),
        )
    }
}
