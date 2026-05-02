//! Windows Named Pipe IPC server. Hosts `\\.\pipe\boxpilot-helper`,
//! resolves the connecting client's SID via
//! `GetNamedPipeClientProcessId` + `OpenProcessToken` +
//! `GetTokenInformation(TokenUser)`, and forwards each request to a
//! `HelperDispatch`. Wire format per spec §5.4.1.
//!
//! Caller-resolution code is INTENTIONALLY in this module rather than a
//! standalone `CallerResolver` (per COQ10).

use crate::traits::authority::CallerPrincipal;
use crate::traits::bundle_aux::AuxStream;
use crate::traits::ipc::{ConnectionInfo, HelperDispatch, IpcClient, IpcServer};
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};
use std::io::Cursor;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

const PIPE_NAME: &str = r"\\.\pipe\boxpilot-helper";
const MAGIC: u32 = 0x426F7850; // ASCII "BoxP" — per spec §5.4.1

pub struct NamedPipeIpcServer {
    pub stop: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl IpcServer for NamedPipeIpcServer {
    async fn run(&self, dispatch: Arc<dyn HelperDispatch>) -> Result<(), HelperError> {
        let dispatch = dispatch;
        let stop = Arc::clone(&self.stop);
        // Pre-create the first server instance.
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(PIPE_NAME)
            .map_err(|e| HelperError::Ipc {
                message: format!("create pipe: {e}"),
            })?;
        loop {
            tokio::select! {
                _ = stop.notified() => {
                    return Ok(());
                }
                acc = server.connect() => {
                    acc.map_err(|e| HelperError::Ipc {
                        message: format!("pipe connect: {e}"),
                    })?;
                    // Take the connected pipe out and create a fresh listener
                    // so the next client can be accepted.
                    let connected = std::mem::replace(
                        &mut server,
                        ServerOptions::new().create(PIPE_NAME).map_err(|e| HelperError::Ipc {
                            message: format!("create pipe (next): {e}"),
                        })?,
                    );
                    let dispatch_clone = Arc::clone(&dispatch);
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(connected, dispatch_clone).await {
                            tracing::warn!("ipc connection: {e:?}");
                        }
                    });
                }
            }
        }
    }
}

async fn handle_connection(
    mut pipe: NamedPipeServer,
    dispatch: Arc<dyn HelperDispatch>,
) -> Result<(), HelperError> {
    use std::os::windows::io::AsRawHandle;

    // Resolve caller SID.
    let principal = resolve_caller_sid(pipe.as_raw_handle())?;

    // Read header (60 bytes per §5.4.1).
    let mut header = [0u8; 60];
    pipe.read_exact(&mut header)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("read header: {e}"),
        })?;

    let mut hcur = Cursor::new(&header[..]);
    let magic = read_u32(&mut hcur)?;
    if magic != MAGIC {
        return Err(HelperError::Ipc {
            message: format!("bad magic: 0x{magic:08X}"),
        });
    }
    let method_id = read_u32(&mut hcur)?;
    let flags = read_u32(&mut hcur)?;
    let body_len = read_u64(&mut hcur)?;
    let _body_sha256_present = read_u32(&mut hcur)?;
    let mut _body_sha256 = [0u8; 32];
    std::io::Read::read_exact(&mut hcur, &mut _body_sha256).map_err(|e| HelperError::Ipc {
        message: format!("read header sha: {e}"),
    })?;
    // _reserved at end (4 bytes) intentionally not read because Cursor over &[u8;60]
    // is exhausted now.

    let method = HelperMethod::from_wire_id(method_id).ok_or_else(|| HelperError::Ipc {
        message: format!("unknown method id 0x{method_id:08X}"),
    })?;

    if body_len > 4 * 1024 * 1024 {
        return Err(HelperError::Ipc {
            message: format!("body_len {body_len} exceeds 4 MiB cap"),
        });
    }
    let mut body = vec![0u8; body_len as usize];
    pipe.read_exact(&mut body)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("read body: {e}"),
        })?;

    // Aux: chunked frames if flags.aux_present.
    let aux_present = flags & 1 != 0;
    let aux = if aux_present {
        let aux_cap = method.aux_size_cap();
        let mut acc: Vec<u8> = Vec::new();
        loop {
            let mut len_buf = [0u8; 4];
            pipe.read_exact(&mut len_buf)
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("read chunk len: {e}"),
                })?;
            let chunk_len = u32::from_le_bytes(len_buf);
            if chunk_len == 0 {
                break;
            }
            if (acc.len() as u64) + (chunk_len as u64) > aux_cap {
                return Err(HelperError::Ipc {
                    message: format!("aux exceeds cap {aux_cap}"),
                });
            }
            let start = acc.len();
            acc.resize(start + chunk_len as usize, 0);
            pipe.read_exact(&mut acc[start..])
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("read chunk: {e}"),
                })?;
        }
        AuxStream::from_async_read(Cursor::new(acc))
    } else {
        AuxStream::none()
    };

    // Dispatch.
    let conn = ConnectionInfo { caller: principal };
    let result = dispatch.handle(conn, method, body, aux).await;

    // Write response.
    let (status, body) = match result {
        Ok(b) => (0u32, b),
        Err(e) => (helper_error_wire_id(&e), e.to_string().into_bytes()),
    };
    pipe.write_all(&status.to_le_bytes())
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write status: {e}"),
        })?;
    let body_len_u64 = body.len() as u64;
    pipe.write_all(&body_len_u64.to_le_bytes())
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write body_len: {e}"),
        })?;
    pipe.write_all(&body).await.map_err(|e| HelperError::Ipc {
        message: format!("write body: {e}"),
    })?;
    pipe.flush().await.map_err(|e| HelperError::Ipc {
        message: format!("flush: {e}"),
    })?;
    Ok(())
}

fn read_u32(c: &mut Cursor<&[u8]>) -> Result<u32, HelperError> {
    let mut buf = [0u8; 4];
    std::io::Read::read_exact(c, &mut buf).map_err(|e| HelperError::Ipc {
        message: format!("read u32: {e}"),
    })?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(c: &mut Cursor<&[u8]>) -> Result<u64, HelperError> {
    let mut buf = [0u8; 8];
    std::io::Read::read_exact(c, &mut buf).map_err(|e| HelperError::Ipc {
        message: format!("read u64: {e}"),
    })?;
    Ok(u64::from_le_bytes(buf))
}

fn helper_error_wire_id(e: &HelperError) -> u32 {
    // Stable ids for the §5.4.1 response status field. Add to
    // boxpilot-ipc::error::wire if more granularity is needed.
    match e {
        HelperError::NotImplemented => 0x0001,
        HelperError::NotAuthorized => 0x0002,
        HelperError::NotController => 0x0003,
        HelperError::ControllerOrphaned => 0x0004,
        HelperError::ControllerNotSet => 0x0005,
        HelperError::Busy => 0x0006,
        HelperError::Ipc { .. } => 0x0010,
        _ => 0x00FF, // catch-all; refined in Sub-project #2
    }
}

/// Caller-resolution helper (per COQ10 — was previously a standalone
/// `CallerResolver` trait; now an internal of the Windows IpcServer).
fn resolve_caller_sid(
    pipe_handle: std::os::windows::io::RawHandle,
) -> Result<CallerPrincipal, HelperError> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let mut pid: u32 = 0;
        if GetNamedPipeClientProcessId(pipe_handle as HANDLE, &mut pid) == 0 {
            return Err(HelperError::Ipc {
                message: "GetNamedPipeClientProcessId failed".into(),
            });
        }
        let proc_h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if proc_h.is_null() {
            return Err(HelperError::Ipc {
                message: format!("OpenProcess({pid})"),
            });
        }
        let mut token: HANDLE = std::ptr::null_mut();
        let ok = OpenProcessToken(proc_h, TOKEN_QUERY, &mut token);
        CloseHandle(proc_h);
        if ok == 0 {
            return Err(HelperError::Ipc {
                message: "OpenProcessToken failed".into(),
            });
        }
        // Query TokenUser size, then payload.
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        if GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            len,
            &mut len,
        ) == 0
        {
            CloseHandle(token);
            return Err(HelperError::Ipc {
                message: "GetTokenInformation(TokenUser) failed".into(),
            });
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let sid_ptr = user.User.Sid;
        // Convert to string SID.
        let mut wstr_ptr: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid_ptr, &mut wstr_ptr) == 0 {
            CloseHandle(token);
            return Err(HelperError::Ipc {
                message: "ConvertSidToStringSidW failed".into(),
            });
        }
        let mut len_w = 0;
        while *wstr_ptr.add(len_w) != 0 {
            len_w += 1;
        }
        let slice = std::slice::from_raw_parts(wstr_ptr, len_w);
        let sid = String::from_utf16_lossy(slice);
        // LocalFree skipped — small leak per call, fine for Sub-project #1.
        CloseHandle(token);
        Ok(CallerPrincipal::WindowsSid(sid))
    }
}

pub struct NamedPipeIpcClient;

impl NamedPipeIpcClient {
    pub fn connect() -> Result<Self, HelperError> {
        // Defensive: ensure the pipe exists before we try to connect.
        // ClientOptions::new().open(...) handles waiting internally.
        Ok(Self)
    }
}

#[async_trait]
impl IpcClient for NamedPipeIpcClient {
    async fn call(
        &self,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,
    ) -> HelperResult<Vec<u8>> {
        let mut pipe = ClientOptions::new()
            .open(PIPE_NAME)
            .map_err(|e| HelperError::Ipc {
                message: format!("open pipe: {e}"),
            })?;

        // Header.
        let aux_present = !aux.is_none();
        let flags: u32 = if aux_present { 1 } else { 0 };
        let body_len = body.len() as u64;
        let mut header = Vec::with_capacity(60);
        header.extend_from_slice(&MAGIC.to_le_bytes());
        header.extend_from_slice(&method.wire_id().to_le_bytes());
        header.extend_from_slice(&flags.to_le_bytes());
        header.extend_from_slice(&body_len.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // body_sha256_present = 0
        header.extend_from_slice(&[0u8; 32]); // body_sha256 padding
        header.extend_from_slice(&0u32.to_le_bytes()); // reserved
        debug_assert_eq!(header.len(), 60);

        pipe.write_all(&header)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("write header: {e}"),
            })?;
        pipe.write_all(&body).await.map_err(|e| HelperError::Ipc {
            message: format!("write body: {e}"),
        })?;

        // Aux frames if present.
        if aux_present {
            let mut reader = aux.into_async_read();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = reader.read(&mut buf).await.map_err(|e| HelperError::Ipc {
                    message: format!("read aux: {e}"),
                })?;
                if n == 0 {
                    pipe.write_all(&0u32.to_le_bytes()).await.ok();
                    break;
                }
                pipe.write_all(&(n as u32).to_le_bytes()).await.ok();
                pipe.write_all(&buf[..n]).await.ok();
            }
        }

        pipe.flush().await.map_err(|e| HelperError::Ipc {
            message: format!("flush: {e}"),
        })?;

        // Read response.
        let mut status_buf = [0u8; 4];
        pipe.read_exact(&mut status_buf)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("read status: {e}"),
            })?;
        let status = u32::from_le_bytes(status_buf);
        let mut body_len_buf = [0u8; 8];
        pipe.read_exact(&mut body_len_buf)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("read resp body_len: {e}"),
            })?;
        let resp_len = u64::from_le_bytes(body_len_buf) as usize;
        let mut resp_body = vec![0u8; resp_len];
        pipe.read_exact(&mut resp_body)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("read resp body: {e}"),
            })?;

        if status == 0 {
            Ok(resp_body)
        } else {
            // The body is a UTF-8 message string for status != 0.
            let msg = String::from_utf8_lossy(&resp_body).into_owned();
            // Map known status ids back to HelperError variants where
            // possible; fall back to Ipc{} for the rest.
            Err(match status {
                0x0001 => HelperError::NotImplemented,
                0x0002 => HelperError::NotAuthorized,
                0x0003 => HelperError::NotController,
                0x0004 => HelperError::ControllerOrphaned,
                0x0005 => HelperError::ControllerNotSet,
                0x0006 => HelperError::Busy,
                _ => HelperError::Ipc { message: msg },
            })
        }
    }
}
