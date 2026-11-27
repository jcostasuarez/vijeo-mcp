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

/// Lanza un programa para el usuario, **sin** atarlo al MCP: sigue abierto cuando el MCP termina. No hereda ningún
/// handle (con `std::process::Command` heredaría los pipes de stdio del MCP y el cliente no vería cerrarse su salida
/// mientras la ventana siguiera abierta: verificado). Pide salir del job en que corra el MCP (si su cliente lo metió
/// en uno) y, si ese job no lo permite, lo lanza igual. Devuelve el PID.
pub fn launch_detached(exe: &std::path::Path, args: &[&std::ffi::OsStr]) -> vijeo_core::VijeoResult<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{CreateProcessW, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW};
    // Como `std::process::Command`: comillas sólo en lo que lleva espacios (rutas; sin comillas dentro).
    let mut line: Vec<u16> = Vec::new();
    for (i, a) in std::iter::once(exe.as_os_str()).chain(args.iter().copied()).enumerate() {
        if i > 0 {
            line.push(u16::from(b' '));
        }
        let quote = i == 0 || a.encode_wide().any(|c| c == u16::from(b' ') || c == u16::from(b'\t'));
        if quote {
            line.push(u16::from(b'"'));
        }
        line.extend(a.encode_wide());
        if quote {
            line.push(u16::from(b'"'));
        }
    }
    line.push(0);
    // Sin STARTF_USESTDHANDLES, Windows le copia a un programa de consola los handles estándar del padre aunque no
    // herede ninguno: se le dan nulos.
    let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, dwFlags: STARTF_USESTDHANDLES, ..Default::default() };
    let mut last = None;
    for flags in [CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP] {
        let mut pi = PROCESS_INFORMATION::default();
        let mut buf = line.clone();
        // SAFETY: `buf` (terminado en 0) vive durante la llamada; `si` y `pi` son estructuras válidas.
        match unsafe { CreateProcessW(PCWSTR::null(), Some(PWSTR(buf.as_mut_ptr())), None, None, false, flags, None, PCWSTR::null(), &si, &mut pi) } {
            Ok(()) => {
                // SAFETY: handles que acaba de devolver CreateProcessW y que no se usan más.
                unsafe {
                    let _ = CloseHandle(pi.hThread);
                    let _ = CloseHandle(pi.hProcess);
                }
                tracing::info!(pid = pi.dwProcessId, "lanzado para el usuario, sin atar al MCP");
                return Ok(pi.dwProcessId);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(vijeo_core::VijeoError::Internal(format!("no se pudo lanzar {}: {}", exe.display(), last.map(|e| e.message()).unwrap_or_default())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// Lanza un ping de unos segundos sin atarlo: sigue vivo al volver y se puede terminar por su PID. Que no retiene los
    /// pipes del que lo lanza se comprueba con `cargo test -p vijeo-ui detached | cat`, que tiene que acabar enseguida.
    #[test]
    fn detached_launch_survives_and_has_a_pid() {
        let ping = std::path::Path::new(r"C:\Windows\System32\PING.EXE");
        let pid = launch_detached(ping, &[std::ffi::OsStr::new("-n"), std::ffi::OsStr::new("8"), std::ffi::OsStr::new("127.0.0.1")]).unwrap();
        assert!(pid != 0 && is_alive(pid));
        assert!(launch_detached(std::path::Path::new(r"C:\no\existe.exe"), &[]).is_err());
    }

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
