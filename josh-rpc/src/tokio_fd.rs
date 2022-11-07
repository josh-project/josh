// Code adapted from
// https://github.com/nanpuyue/tokio-fd/blob/b4730113ca937152c2b106bb490c7b242aec2c81/src/lib.rs
// (Apache 2.0, MIT)

use std::convert::TryFrom;
use std::pin::Pin;
use std::task::{Context, Poll, Poll::*};
use std::os::unix::io::AsRawFd;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

type UnixRawFd = std::os::unix::io::RawFd;

pub struct AsyncFd {
    raw_fd: tokio::io::unix::AsyncFd<UnixRawFd>
}

impl TryFrom<UnixRawFd> for AsyncFd {
    type Error = std::io::Error;

    fn try_from(fd: UnixRawFd) -> std::io::Result<Self> {
        fd_set_nonblock(fd)?;

        Ok(Self {
            raw_fd: tokio::io::unix::AsyncFd::new(fd)?
        })
    }
}

impl AsRawFd for AsyncFd {
    fn as_raw_fd(&self) -> UnixRawFd {
        *self.raw_fd.get_ref()
    }
}

impl AsyncRead for AsyncFd {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        loop {
            let mut ready = match self.raw_fd.poll_read_ready(cx) {
                Ready(result) => result?,
                Pending => return Pending,
            };

            let ret = unsafe {
                libc::read(
                    self.as_raw_fd(),
                    buf.unfilled_mut() as *mut _ as _,
                    buf.remaining(),
                )
            };

            return if ret < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    ready.clear_ready();
                    continue;
                } else {
                    Ready(Err(e))
                }
            } else {
                let n = ret as usize;
                unsafe { buf.assume_init(n) };
                buf.advance(n);
                Ready(Ok(()))
            };
        }
    }
}

impl AsyncWrite for AsyncFd {
    fn poll_write(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &[u8]
    ) -> Poll<std::io::Result<usize>> {
        loop {
            let mut ready = match self.raw_fd.poll_write_ready(ctx) {
                Ready(result) => result?,
                Pending => return Pending,
            };

            let ret = unsafe {
                libc::write(self.as_raw_fd(), buf.as_ptr() as _, buf.len())
            };

            match ret {
                _ if ret < 0 => {
                    match std::io::Error::last_os_error() {
                        e if e.kind() == std::io::ErrorKind::WouldBlock => {
                            ready.clear_ready();
                            continue
                        }
                        e => return Ready(Err(e))
                    }
                },
                _ => return Ready(Ok(ret as usize))
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Ready(Ok(()))
    }
}

fn fd_set_nonblock(fd: UnixRawFd) -> std::io::Result<()> {
    let flags = unsafe {
        libc::fcntl(fd, libc::F_GETFL)
    };

    match flags {
        error if error < 0 => Err(std::io::Error::last_os_error()),
        flags => {
            let set_result = unsafe {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK)
            };

            match set_result {
                0 => Ok(()),
                _ => Err(std::io::Error::last_os_error()),
            }
        }
    }
}
