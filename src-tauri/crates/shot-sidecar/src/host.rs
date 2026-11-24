use std::fs::File;
use std::os::windows::io::FromRawHandle;

use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree, DUPLICATE_SAME_ACCESS, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SYNCHRONIZE,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows_sys::Win32::System::IO::CancelSynchronousIo;
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, GetCurrentProcess, OpenProcess, OpenThread, SetEvent,
    WaitForMultipleObjects, INFINITE,
};

use crate::log;

const SDDL_REVISION_1: u32 = 1;
const THREAD_TERMINATE: u32 = 0x0001;

pub struct OwnedHandle(pub HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn lock_single_instance(name: &str) -> Result<OwnedHandle, String> {
    match create_mutex(name) {
        Ok(handle) => Ok(OwnedHandle(handle)),
        Err(MutexError::Denied) if name == shot_core::MUTEX_NAME => {
            log::warn("全局互斥锁没有权限，改用当前会话的锁");
            lock_single_instance(shot_core::MUTEX_NAME_LOCAL)
        }
        Err(MutexError::Exists) => Err("截图进程已经在运行".into()),
        Err(MutexError::Denied) => Err("创建截图进程互斥锁被拒绝".into()),
        Err(MutexError::Other(err)) => Err(err),
    }
}

enum MutexError {
    Exists,
    Denied,
    Other(String),
}

fn create_mutex(name: &str) -> Result<HANDLE, MutexError> {
    let wide_name = wide(name);
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, wide_name.as_ptr()) };
    if handle.is_null() {
        return Err(if unsafe { GetLastError() } == 5 {
            MutexError::Denied
        } else {
            MutexError::Other(format!(
                "创建互斥锁失败: {}",
                std::io::Error::last_os_error()
            ))
        });
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle) };
        return Err(MutexError::Exists);
    }
    Ok(handle)
}

pub fn create_pipe(name: &str) -> Result<HANDLE, String> {
    let wide_name = wide(name);
    let security = user_security()?;
    let handle = unsafe {
        CreateNamedPipeW(
            wide_name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            security.as_ptr(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(format!(
            "创建命名管道失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(handle)
}

struct UserSecurity {
    descriptor: *mut std::ffi::c_void,
    attributes: SECURITY_ATTRIBUTES,
}

impl UserSecurity {
    fn as_ptr(&self) -> *const SECURITY_ATTRIBUTES {
        &self.attributes
    }
}

impl Drop for UserSecurity {
    fn drop(&mut self) {
        if !self.descriptor.is_null() {
            unsafe { LocalFree(self.descriptor) };
        }
    }
}

fn user_security() -> Result<UserSecurity, String> {
    let sid = current_user_sid()?;
    let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
    let mut descriptor = std::ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 || descriptor.is_null() {
        return Err(format!(
            "设置管道权限失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    Ok(UserSecurity {
        descriptor,
        attributes,
    })
}

fn current_user_sid() -> Result<String, String> {
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::OpenProcessToken;

    let mut token: HANDLE = std::ptr::null_mut();
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 {
        return Err(format!(
            "读取当前用户失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    let token = OwnedHandle(token);
    let mut needed = 0u32;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if needed == 0 {
        return Err(format!(
            "读取当前用户失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut buffer = vec![0u8; needed as usize];
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(format!(
            "读取当前用户失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    let mut raw = std::ptr::null_mut();
    let converted = unsafe { ConvertSidToStringSidW(user.User.Sid, &mut raw) };
    if converted == 0 || raw.is_null() {
        return Err(format!(
            "转换当前用户 SID 失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut len = 0usize;
    unsafe {
        while *raw.add(len) != 0 {
            len += 1;
            if len > 256 {
                break;
            }
        }
    }
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw, len) });
    unsafe { LocalFree(raw.cast()) };
    if sid.is_empty() {
        return Err("当前用户 SID 是空的".into());
    }
    Ok(sid)
}

pub fn connect_pipe(handle: HANDLE) -> Result<(), String> {
    let ok = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if ok != 0 {
        return Ok(());
    }
    if unsafe { GetLastError() } == ERROR_PIPE_CONNECTED {
        return Ok(());
    }
    Err(format!(
        "等待主进程连接失败: {}",
        std::io::Error::last_os_error()
    ))
}

pub fn duplicate(handle: HANDLE) -> Result<HANDLE, String> {
    let mut copied: HANDLE = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut copied,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 || copied.is_null() {
        return Err(format!(
            "复制管道句柄失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(copied)
}

pub fn file_from(handle: HANDLE) -> File {
    unsafe { File::from_raw_handle(handle as _) }
}

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

#[cfg_attr(not(test), allow(dead_code))]
pub fn connect_client(name: &str) -> Result<File, String> {
    let wide_name = wide(name);
    let mut last = std::io::Error::other("管道还没准备好");
    for _ in 0..40 {
        let handle = unsafe {
            CreateFileW(
                wide_name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE && !handle.is_null() {
            return Ok(file_from(handle));
        }
        last = std::io::Error::last_os_error();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err(format!("连接截图管道失败: {last}"))
}

pub fn open_parent(pid: u32) -> Result<OwnedHandle, String> {
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return Err(format!(
            "打不开父进程 {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(OwnedHandle(handle))
}

pub fn manual_event() -> Result<OwnedHandle, String> {
    let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if handle.is_null() {
        return Err(format!(
            "创建退出事件失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(OwnedHandle(handle))
}

pub fn signal(handle: HANDLE) {
    unsafe { SetEvent(handle) };
}

/// 父进程退出返回 true。`stop` 被点亮时返回 false，调用方继续收尾。
pub fn wait_parent_or_stop(parent: HANDLE, stop: HANDLE) -> bool {
    let handles = [parent, stop];
    let woken = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
    woken == WAIT_OBJECT_0
}

pub fn cancel_thread_io(thread_id: u32) {
    let handle = unsafe { OpenThread(THREAD_TERMINATE, 0, thread_id) };
    if handle.is_null() {
        return;
    }
    unsafe {
        CancelSynchronousIo(handle);
        CloseHandle(handle);
    }
}
