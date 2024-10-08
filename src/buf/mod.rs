//! Utilities for working with buffers.
//!
//! `io-uring` APIs require passing ownership of buffers to the runtime. The
//! crate defines [`IoBuf`] and [`IoBufMut`] traits which are implemented by buffer
//! types that respect the `io-uring` contract.

pub mod fixed;

mod io_buf;
use std::{
    convert::TryFrom,
    iter::zip,
    mem::ManuallyDrop,
    ops::{Index, IndexMut},
};

pub use io_buf::IoBuf;

mod io_buf_mut;
pub use io_buf_mut::IoBufMut;

mod slice;
pub use slice::Slice;

mod bounded;
pub use bounded::{BoundedBuf, BoundedBufMut};

use crate::Error;

pub(crate) fn deref(buf: &impl IoBuf) -> &[u8] {
    // Safety: the `IoBuf` trait is marked as unsafe and is expected to be
    // implemented correctly.
    unsafe { std::slice::from_raw_parts(buf.stable_ptr(), buf.bytes_init()) }
}

pub(crate) fn deref_mut(buf: &mut impl IoBufMut) -> &mut [u8] {
    // Safety: the `IoBufMut` trait is marked as unsafe and is expected to be
    // implemented correct.
    unsafe { std::slice::from_raw_parts_mut(buf.stable_mut_ptr(), buf.bytes_init()) }
}

#[derive(Debug, PartialEq)]
enum BufferSource {
    RawPtr,
    Vector,
}

#[allow(missing_docs)]
pub struct Buffer {
    iovecs: Vec<libc::iovec>,
    state: Vec<BufferState>,
}

unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

impl Buffer {
    fn new(iovecs: Vec<libc::iovec>, state: Vec<BufferState>) -> Self {
        Buffer { iovecs, state }
    }

    #[allow(missing_docs)]
    pub fn len(&self) -> usize {
        self.iovecs.len()
    }

    #[allow(missing_docs)]
    pub fn fill(&mut self) {
        for (iovec, state) in zip(&mut self.iovecs, &self.state) {
            iovec.iov_len = state.total_bytes;
        }
    }

    #[allow(missing_docs)]
    pub unsafe fn from_raw_ptr(ptr: u64, len: usize) -> Self {
        let iov = libc::iovec {
            iov_base: ptr as _,
            iov_len: len,
        };
        let state = BufferState::new(len, fake_drop_vec, BufferSource::RawPtr);
        Self::new(vec![iov], vec![state])
    }
}

#[derive(Debug)]
pub(crate) struct BufferState {
    total_bytes: usize,
    dtor: unsafe fn(libc::iovec, usize),
    source: BufferSource,
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let Self {
            iovecs: iovec,
            state,
        } = self;
        for i in 0..iovec.len() {
            unsafe { (state[i].dtor)(iovec[i], state[i].total_bytes) }
        }
    }
}

impl BufferState {
    fn new(total_bytes: usize, dtor: unsafe fn(libc::iovec, usize), source: BufferSource) -> Self {
        BufferState {
            total_bytes,
            dtor,
            source,
        }
    }
}

impl From<Vec<u8>> for Buffer {
    fn from(buf: Vec<u8>) -> Self {
        let mut vec = ManuallyDrop::new(buf);
        let base = vec.as_mut_ptr();
        let iov_len = vec.len();
        let total_bytes = vec.capacity();

        let iov = libc::iovec {
            iov_base: base as _,
            iov_len,
        };

        let state = BufferState::new(total_bytes, drop_vec, BufferSource::Vector);
        Buffer::new(vec![iov], vec![state])
    }
}

impl From<Vec<Vec<u8>>> for Buffer {
    fn from(bufs: Vec<Vec<u8>>) -> Self {
        let mut iovecs = Vec::with_capacity(bufs.len());
        let mut states = Vec::with_capacity(bufs.len());

        for buf in bufs {
            let mut vec = ManuallyDrop::new(buf);

            let base = vec.as_mut_ptr();
            let iov_len = vec.len();
            let total_bytes = vec.capacity();

            let iov = libc::iovec {
                iov_base: base as *mut libc::c_void,
                iov_len,
            };

            let state = BufferState::new(total_bytes, drop_vec, BufferSource::Vector);

            iovecs.push(iov);
            states.push(state);
        }

        Buffer::new(iovecs, states)
    }
}

impl TryFrom<Buffer> for Vec<u8> {
    type Error = Error<Buffer>;

    fn try_from(buf: Buffer) -> Result<Self, Self::Error> {
        if buf.len() != 1 {
            return Err(Error(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "length of vector of this Buffer must be 1",
                ),
                buf,
            ));
        }

        if buf.state[0].source != BufferSource::Vector {
            return Err(Error(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "the source of this Buffer is not Vec",
                ),
                buf,
            ));
        }

        let this = ManuallyDrop::new(buf);
        Ok(unsafe {
            Vec::from_raw_parts(
                this.iovecs[0].iov_base as _,
                this.iovecs[0].iov_len,
                this.state[0].total_bytes,
            )
        })
    }
}

impl TryFrom<Buffer> for Vec<Vec<u8>> {
    type Error = Error<Buffer>;

    fn try_from(buf: Buffer) -> Result<Self, Self::Error> {
        if buf
            .state
            .iter()
            .any(|state| state.source != BufferSource::Vector)
        {
            return Err(Error(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "the source of any vector of this Buffer is not Vec",
                ),
                buf,
            ));
        }

        let this = ManuallyDrop::new(buf);
        let mut vecs = Vec::with_capacity(this.iovecs.len());
        for i in 0..this.iovecs.len() {
            vecs.push(unsafe {
                Vec::from_raw_parts(
                    this.iovecs[i].iov_base as _,
                    this.iovecs[i].iov_len,
                    this.state[i].total_bytes,
                )
            });
        }
        Ok(vecs)
    }
}

unsafe fn fake_drop_vec(_iovec: libc::iovec, _total_bytes: usize) {}

unsafe fn drop_vec(iovec: libc::iovec, total_bytes: usize) {
    Vec::from_raw_parts(iovec.iov_base as _, iovec.iov_len, total_bytes);
}

impl Index<usize> for Buffer {
    type Output = [u8];

    fn index(&self, index: usize) -> &Self::Output {
        let iovec = &self.iovecs[index];
        unsafe { std::slice::from_raw_parts(iovec.iov_base as *const u8, iovec.iov_len) }
    }
}

impl IndexMut<usize> for Buffer {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let iovec = &mut self.iovecs[index];
        unsafe { std::slice::from_raw_parts_mut(iovec.iov_base as *mut u8, iovec.iov_len) }
    }
}

unsafe impl IoBuf for Buffer {
    fn stable_ptr(&self) -> *const u8 {
        if self.state.len() == 1 {
            self.iovecs[0].iov_base as *const u8
        } else {
            self.iovecs.as_ptr() as *const u8
        }
    }

    fn bytes_init(&self) -> usize {
        self.iovecs.iter().map(|iovec| iovec.iov_len).sum()
    }

    fn bytes_total(&self) -> usize {
        self.state.iter().map(|state| state.total_bytes).sum()
    }
}

unsafe impl IoBufMut for Buffer {
    fn stable_mut_ptr(&mut self) -> *mut u8 {
        if self.state.len() == 1 {
            self.iovecs[0].iov_base as *mut u8
        } else {
            self.iovecs.as_mut_ptr() as *mut u8
        }
    }

    unsafe fn set_init(&mut self, mut pos: usize) {
        for (iovec, state) in zip(&mut self.iovecs, &self.state) {
            let size = std::cmp::min(state.total_bytes, pos);
            iovec.iov_len = size;
            pos -= size;
        }
    }
}
