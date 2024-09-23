use io_uring::cqueue::Entry;

use crate::buf::fixed::FixedBuf;
use crate::buf::BoundedBufMut;
use crate::io::SharedFd;
use crate::{OneshotOutputTransform, Result, UnsubmittedOneshot, WithBuffer};

use std::io;
use std::marker::PhantomData;

/// An unsubmitted read operation.
pub type UnsubmittedRead<T> = UnsubmittedOneshot<ReadData<T>, ReadTransform<T>>;

#[allow(missing_docs)]
pub struct ReadData<T> {
    /// Holds a strong ref to the FD, preventing the file from being closed
    /// while the operation is in-flight.
    _fd: SharedFd,

    buf: T,
}

#[allow(missing_docs)]
pub struct ReadTransform<T> {
    _phantom: PhantomData<T>,
}

impl<T> OneshotOutputTransform for ReadTransform<T>
where
    T: BoundedBufMut,
{
    type Output = Result<usize, T>;
    type StoredData = ReadData<T>;

    fn transform_oneshot_output(self, mut data: Self::StoredData, cqe: Entry) -> Self::Output {
        let n = cqe.result();
        let res = if n >= 0 {
            // Safety: the kernel wrote `n` bytes to the buffer.
            unsafe { data.buf.set_init(n as usize) };
            Ok(n as usize)
        } else {
            Err(io::Error::from_raw_os_error(-n))
        };

        res.with_buffer(data.buf)
    }
}

impl<T: BoundedBufMut> UnsubmittedRead<T> {
    pub(crate) fn read_at(fd: &SharedFd, mut buf: T, offset: u64) -> Self {
        use io_uring::{opcode, types};

        // Get raw buffer info
        let ptr = buf.stable_mut_ptr();
        let len = buf.bytes_total();

        Self::new(
            ReadData {
                _fd: fd.clone(),
                buf,
            },
            ReadTransform {
                _phantom: PhantomData,
            },
            opcode::Read::new(types::Fd(fd.raw_fd()), ptr, len as _)
                .offset(offset as _)
                .build(),
        )
    }
}

impl<T: BoundedBufMut<BufMut = FixedBuf>> UnsubmittedRead<T> {
    pub(crate) fn read_fixed_at(fd: &SharedFd, mut buf: T, offset: u64) -> Self {
        use io_uring::{opcode, types};

        // Get raw buffer info
        let ptr = buf.stable_mut_ptr();
        let len = buf.bytes_total();
        let buf_index = buf.get_buf().buf_index();
        Self::new(
            ReadData {
                _fd: fd.clone(),
                buf,
            },
            ReadTransform {
                _phantom: PhantomData,
            },
            opcode::ReadFixed::new(types::Fd(fd.raw_fd()), ptr, len as _, buf_index)
                .offset(offset as _)
                .build(),
        )
    }
}

impl<T: BoundedBufMut> UnsubmittedRead<T> {
    pub(crate) fn read_fixed_at_with_index(
        fd: &SharedFd,
        mut buf: T,
        buf_index: u16,
        offset: u64,
    ) -> Self {
        use io_uring::{opcode, types};

        // Get raw buffer info
        let ptr = buf.stable_mut_ptr();
        let len = buf.bytes_total();
        Self::new(
            ReadData {
                _fd: fd.clone(),
                buf,
            },
            ReadTransform {
                _phantom: PhantomData,
            },
            opcode::ReadFixed::new(types::Fd(fd.raw_fd()), ptr, len as _, buf_index)
                .offset(offset as _)
                .build(),
        )
    }
}
