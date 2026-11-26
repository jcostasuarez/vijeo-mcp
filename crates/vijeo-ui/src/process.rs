//! Procesos de Vijeo: localizar por nombre, comprobar si siguen vivos, matarlos y atarlos al MCP.

use std::collections::HashSet;
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

pub const VIJEO_FRAME_EXE: &str = "Vijeo-Frame.exe";

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcEntry {
    pub pid: u32,
    pub parent: u32,
    pub exe: String,
}

/// Instantánea de todos los procesos.
pub fn snapshot() -> Vec<ProcEntry> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let snap = Handle(snap);
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap.0, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.push(ProcEntry { pid: e.th32ProcessID, parent: e.th32ParentProcessID, exe: String::from_utf16_lossy(&e.szExeFile[..len]) });
            ok = Process32NextW(snap.0, &mut e).is_ok();
        }
    }
    out
}

/// PIDs de los procesos cuyo ejecutable se llama `exe` (sin distinguir mayúsculas).
pub fn pids_by_name(exe: &str) -> HashSet<u32> {
    snapshot().into_iter().filter(|p| p.exe.eq_ignore_ascii_case(exe)).map(|p| p.pid).collect()
}

pub fn is_alive(pid: u32) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let h = Handle(h);
        let mut code = 0u32;
        GetExitCodeProcess(h.0, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32
    }
}

/// Termina el proceso y espera hasta 5 s a que desaparezca. Devuelve si quedó terminado.
pub fn kill(pid: u32) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) else {
            return !is_alive(pid);
        };
        let h = Handle(h);
        let _ = TerminateProcess(h.0, 1);
        let _ = WaitForSingleObject(h.0, 5000);
    }
    !is_alive(pid)
}

/// Job object con `KILL_ON_JOB_CLOSE`: Windows termina sus procesos (y los que éstos lancen) cuando se
/// cierra su último handle, también si el MCP muere de golpe. El handle se guarda como entero: los
/// `HANDLE` no son `Send`.
pub struct Job(isize);

impl Job {
    pub fn new() -> Option<Self> {
        unsafe {
            let h = CreateJobObjectW(None, PCWSTR::null()).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set.is_err() {
                let _ = CloseHandle(h);
                return None;
            }
            Some(Self(h.0 as isize))
        }
    }

    /// Mete el proceso en el job. Falla si ya está en un job que no admite anidar (Windows 7 o anterior).
    pub fn assign(&self, process: HANDLE) -> bool {
        unsafe { AssignProcessToJobObject(HANDLE(self.0 as *mut _), process) }.is_ok()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(HANDLE(self.0 as *mut _));
        }
    }
}

/// Ata el proceso al MCP: su job no se cierra nunca (los `static` no se sueltan), así que Windows lo
/// termina cuando termina el MCP, sea como sea. `false` si no se pudo.
pub fn tie_to_this_process(process: HANDLE) -> bool {
    static JOB: OnceLock<Option<Job>> = OnceLock::new();
    JOB.get_or_init(|| {
        let job = Job::new();
        if job.is_none() {
            tracing::warn!("no se pudo crear el job object: las instancias de Vijeo sobrevivirán al MCP si muere de golpe");
        }
        job
    })
    .as_ref()
    .is_some_and(|j| j.assign(process))
}

/// [`tie_to_this_process`] para un proceso lanzado con `std::process::Command`.
pub fn tie_child(child: &std::process::Child) -> bool {
    use std::os::windows::io::AsRawHandle;
    tie_to_this_process(HANDLE(child.as_raw_handle()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn closing_the_job_kills_its_processes() {
        let mut child = Command::new("ping.exe").args(["-n", "60", "127.0.0.1"]).stdout(Stdio::null()).spawn().unwrap();
        let job = Job::new().expect("job object");
        assert!(job.assign(HANDLE(child.as_raw_handle())));
        assert!(child.try_wait().unwrap().is_none());
        drop(job);
        let t = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if t.elapsed() > Duration::from_secs(5) {
                let _ = child.kill();
                panic!("el proceso sigue vivo tras cerrar el job");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn snapshot_sees_current_process() {
        let me = std::process::id();
        let snap = snapshot();
        assert!(snap.iter().any(|e| e.pid == me && e.exe.to_ascii_lowercase().ends_with(".exe")));
    }
}
