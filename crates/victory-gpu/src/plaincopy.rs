//! Copies of title memory made in place, by this process, with a way back
//! when a page is not there (`VICTORY_COPY_PLAIN=1`; off by default).
//!
//! Every copy of title memory into a host buffer goes through the kernel
//! (`ReadProcessMemory` on our own process, `crate::host::copy_mapped`):
//! the title unmaps and remaps its memory while we read it, a read in
//! place of a page that is gone is an access violation, and the kernel's
//! copy fails instead. It costs what a system call that locks and probes
//! every page costs: run 225's profile has the submitting thread 35 ms a
//! frame inside `ZwReadVirtualMemory` for about 100 MB (3 GB a second),
//! where the processor's own copy moves that much in a third of the time.
//!
//! Here the copy is one `rep movsb` in a function of ours that uses no
//! stack. An exception handler, first in the line, looks at every access
//! violation: one that is a READ and happened at exactly that instruction
//! continues at the function's exit, which returns the bytes left, and the
//! caller falls back to the kernel's copy as before (it says which page
//! is gone and fills in zeros). A write that faults is not ours to take:
//! the destination is host memory, which is always there; such a fault
//! goes on to the handlers after this one.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".globl victory_copy_guarded",
    ".globl victory_copy_guarded_at",
    ".globl victory_copy_guarded_fault",
    ".globl victory_write_guarded",
    ".globl victory_write_guarded_at",
    ".globl victory_write_guarded_fault",
    "victory_copy_guarded:",
    // rcx: to, rdx: from, r8: bytes. rsi and rdi are the caller's: kept in
    // r9 and r10, which are ours to use, so nothing is on the stack.
    "    mov r9, rsi",
    "    mov r10, rdi",
    "    mov rdi, rcx",
    "    mov rsi, rdx",
    "    mov rcx, r8",
    "victory_copy_guarded_at:",
    "    rep movsb",
    "victory_copy_guarded_fault:",
    // RCX is zero on success or the number of bytes left at the fault.
    "    mov rax, rcx",
    "    mov rsi, r9",
    "    mov rdi, r10",
    "    ret",
    "victory_write_guarded:",
    "    mov r9, rsi",
    "    mov r10, rdi",
    "    mov rdi, rcx",
    "    mov rsi, rdx",
    "    mov rcx, r8",
    "victory_write_guarded_at:",
    "    rep movsb",
    "    mov rsi, r9",
    "    mov rdi, r10",
    "    mov eax, 1",
    "    ret",
    "victory_write_guarded_fault:",
    "    mov rsi, r9",
    "    mov rdi, r10",
    "    xor eax, eax",
    "    ret",
);

#[cfg(target_arch = "x86_64")]
unsafe extern "C" {
    fn victory_copy_guarded(to: *mut u8, from: *const u8, len: usize) -> usize;
    static victory_copy_guarded_at: u8;
    static victory_copy_guarded_fault: u8;
    fn victory_write_guarded(to: *mut u8, from: *const u8, len: usize) -> u32;
    static victory_write_guarded_at: u8;
    static victory_write_guarded_fault: u8;
}

/// Whether copies of title memory into host memory are made in place
/// (`VICTORY_COPY_PLAIN=1`).
pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = cfg!(target_arch = "x86_64") && std::env::var_os("VICTORY_COPY_PLAIN").is_some_and(|v| v == "1");
        if on {
            install();
            tracing::info!(
                "copies of title memory into host buffers are made in place, with the kernel's copy behind them for a \
                 page that is not there (VICTORY_COPY_PLAIN=1)"
            );
        }
        on
    })
}

/// Copies made in place, their bytes, the copies that met a page that was
/// not there (and went to the kernel's copy), and the copies not tried in
/// place because their source faulted lately ([`RECENT`]) or they were
/// under [`LEAST`] bytes, since the last line.
static COUNT: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

/// The fewest bytes copied in place. A read of a byte or a word is how the
/// layer ASKS whether a page is there (a texture's first page, a record):
/// it is expected to fail, and a failed kernel read costs a microsecond
/// where a fault and its handler cost tens (run 230: 1,000 to 1,300 faults
/// a frame, nearly all of them such askings, took back most of what the
/// copies in place had won).
const LEAST: usize = 256;

/// The 64 KiB blocks where a copy in place stopped at a fault:
/// their copies go straight to the kernel's, which reads round the hole.
/// By the block's number, in the place its low bits name (a later block
/// with the same low bits takes the place). Forgotten every two seconds:
/// the title maps the memory in the end.
static RECENT: RecentFaults = RecentFaults::new();
/// When [`RECENT`] was last emptied (milliseconds of [`now_ms`]).
static EMPTIED: AtomicU64 = AtomicU64::new(0);

struct RecentFaults([AtomicUsize; 1024]);

impl RecentFaults {
    const fn new() -> Self {
        Self([const { AtomicUsize::new(0) }; 1024])
    }

    fn block(address: usize) -> usize {
        (address >> 16) + 1 // zero marks an empty slot
    }

    fn remember(&self, address: usize) {
        let block = Self::block(address);
        self.0[block % self.0.len()].store(block, Relaxed);
    }

    fn contains(&self, from: usize, len: usize) -> bool {
        let first = Self::block(from);
        let last = Self::block(from.saturating_add(len - 1));
        if last - first < self.0.len() {
            (first..=last).any(|block| self.0[block % self.0.len()].load(Relaxed) == block)
        } else {
            // Large copies do not need to probe the same cache slot twice.
            self.0.iter().any(|slot| (first..=last).contains(&slot.load(Relaxed)))
        }
    }
}

fn now_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

fn expire_recent_faults() {
    let now = now_ms();
    let emptied = EMPTIED.load(Relaxed);
    if now.saturating_sub(emptied) >= 2_000 && EMPTIED.compare_exchange(emptied, now, Relaxed, Relaxed).is_ok() {
        for place in &RECENT.0 {
            place.store(0, Relaxed);
        }
    }
}

/// The handler that lets [`guarded`] return on a read fault. Installed first
/// in the chain, once.
#[cfg(target_arch = "x86_64")]
fn install() {
    use windows_sys::Win32::Foundation::{
        EXCEPTION_ACCESS_VIOLATION, EXCEPTION_GUARD_PAGE, EXCEPTION_IN_PAGE_ERROR,
    };
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS,
    };
    unsafe extern "system" fn on_fault(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: the system passes the exception being dispatched and the
        // context it will continue with.
        let (r, context) = unsafe { (&*(*info).ExceptionRecord, &mut *(*info).ContextRecord) };
        // Only at our own copy/write instructions.
        if context.Rip == (&raw const victory_copy_guarded_at) as u64 {
            const READ: usize = 0;
            let read = r.NumberParameters >= 1 && r.ExceptionInformation[0] == READ;
            let gone = (r.ExceptionCode == EXCEPTION_ACCESS_VIOLATION || r.ExceptionCode == EXCEPTION_IN_PAGE_ERROR) && read;
            if gone || r.ExceptionCode == EXCEPTION_GUARD_PAGE {
                context.Rip = (&raw const victory_copy_guarded_fault) as u64;
                return EXCEPTION_CONTINUE_EXECUTION;
            }
            return EXCEPTION_CONTINUE_SEARCH;
        }
        if context.Rip == (&raw const victory_write_guarded_at) as u64 {
            const WRITE: usize = 1;
            let write = r.NumberParameters >= 1 && r.ExceptionInformation[0] == WRITE;
            let gone = (r.ExceptionCode == EXCEPTION_ACCESS_VIOLATION || r.ExceptionCode == EXCEPTION_IN_PAGE_ERROR) && write;
            if gone || r.ExceptionCode == EXCEPTION_GUARD_PAGE {
                context.Rip = (&raw const victory_write_guarded_fault) as u64;
                return EXCEPTION_CONTINUE_EXECUTION;
            }
        }
        EXCEPTION_CONTINUE_SEARCH
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: a handler that reads the record it is given and nothing else.
    ONCE.call_once(|| unsafe {
        AddVectoredExceptionHandler(1, Some(on_fault));
    });
}

#[cfg(not(target_arch = "x86_64"))]
fn install() {}

/// Copies `len` bytes from `from` to `to` in place. Returns the bytes the
/// copy did not reach when the source faulted, or zero on success.
///
/// `to` must be writable memory of `len` bytes that is ours (host memory);
/// `from` may be anything.
#[cfg(target_arch = "x86_64")]
fn guarded(to: *mut u8, from: *const u8, len: usize) -> usize {
    install();
    // SAFETY: the destination is the caller's to write; a source page that
    // is not there ends the copy at our exception handler ([`install`]).
    unsafe { victory_copy_guarded(to, from, len) }
}

#[cfg(not(target_arch = "x86_64"))]
fn guarded(_: *mut u8, _: *const u8, len: usize) -> usize {
    len
}

/// Copies `len` bytes from host memory `from` into title memory `to` in place;
/// false if a destination page is not mapped or not writable.
#[cfg(target_arch = "x86_64")]
fn guarded_write(to: *mut u8, from: *const u8, len: usize) -> bool {
    install();
    // SAFETY: `from` is our readable host buffer; any unmapped or read-only
    // destination page ends the write with 0 via [`install`].
    unsafe { victory_write_guarded(to, from, len) != 0 }
}

#[cfg(not(target_arch = "x86_64"))]
fn guarded_write(_: *mut u8, _: *const u8, _: usize) -> bool {
    false
}

fn write_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| on() && std::env::var_os("VICTORY_WRITE_PLAIN").is_none_or(|v| v != "0"))
}

/// Writes made in place, their bytes, and the writes that met an unmapped or
/// read-only destination page (falling back to the kernel's write).
static WRITE_COUNT: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

/// With [`on`] (and `VICTORY_WRITE_PLAIN!=0`): writes host memory at `from`
/// into title memory at `to` in place; false if disabled or a destination page
/// is not writable (caller falls back to `NtWriteVirtualMemory`).
pub(crate) fn write(to: *mut u8, from: *const u8, len: usize) -> bool {
    if !write_on() {
        return false;
    }
    let whole = guarded_write(to, from, len);
    if whole {
        WRITE_COUNT[0].fetch_add(1, Relaxed);
        WRITE_COUNT[1].fetch_add(len as u64, Relaxed);
    } else {
        WRITE_COUNT[2].fetch_add(1, Relaxed);
    }
    whole
}

/// With [`on`]: copies title memory at `from` into host memory at `to`;
/// false if it is off or a source page was not there (the caller then
/// copies through the kernel, as before).
pub(crate) fn copy(to: *mut u8, from: *const u8, len: usize) -> bool {
    if !on() {
        return false;
    }
    if len < LEAST {
        COUNT[4].fetch_add(1, Relaxed);
        return false;
    }
    expire_recent_faults();
    copy_unless_recent_fault(to, from, len, &RECENT)
}

fn copy_unless_recent_fault(to: *mut u8, from: *const u8, len: usize, recent: &RecentFaults) -> bool {
    if recent.contains(from as usize, len) {
        COUNT[3].fetch_add(1, Relaxed);
        return false;
    }
    let remaining = guarded(to, from, len);
    if remaining == 0 {
        COUNT[0].fetch_add(1, Relaxed);
        COUNT[1].fetch_add(len as u64, Relaxed);
    } else {
        COUNT[2].fetch_add(1, Relaxed);
        // REP MOVSB stops with the faulting byte still to be copied.
        recent.remember(from.wrapping_add(len - remaining) as usize);
    }
    remaining == 0
}

/// With [`on`]: copies title memory at `from` into host memory at `to` for
/// bytes that are all real, however few (a texture's last pixels, a buffer
/// written back): no [`LEAST`] floor, since only the reads that ask whether a
/// page is there, which are expected to fail, keep it ([`copy`]). False if it
/// is off or a source page was not there (the caller then copies through the
/// kernel, as before).
pub(crate) fn copy_buffer(to: *mut u8, from: *const u8, len: usize) -> bool {
    if !on() {
        return false;
    }
    if len == 0 {
        return true;
    }
    if faulted_lately(from as usize, len) {
        COUNT[3].fetch_add(1, Relaxed);
        return false;
    }
    let whole = guarded(to, from, len);
    if whole {
        COUNT[0].fetch_add(1, Relaxed);
        COUNT[1].fetch_add(len as u64, Relaxed);
    } else {
        COUNT[2].fetch_add(1, Relaxed);
        note_fault(from as usize, len);
    }
    whole
}

/// With the frame line: what [`copy`] did per frame over `frames`.
pub(crate) fn line(frames: f64) {
    if !on() {
        return;
    }
    let [copies, bytes, gone, lately, small] = [0, 1, 2, 3, 4].map(|i| COUNT[i].swap(0, Relaxed) as f64 / frames);
    let [writes, wbytes, wgone] = [0, 1, 2].map(|i| WRITE_COUNT[i].swap(0, Relaxed) as f64 / frames);
    tracing::info!(
        "copies of title memory made in place per frame (VICTORY_COPY_PLAIN=1): {copies:.0}, {:.1} MB; {gone:.1} met a page \
         that was not there and went through the kernel's copy; left to the kernel's copy untried: {lately:.0} whose source \
         faulted in the last two seconds, {small:.0} of under {LEAST} bytes; writes back in place: {writes:.0}, {:.1} MB, \
         {wgone:.1} fell back",
        bytes / 1e6,
        wbytes / 1e6
    );
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc, VirtualFree,
    };

    const PAGE: usize = 4096;

    #[test]
    fn memory_that_is_there_is_copied() {
        let from: Vec<u8> = (0..100_000u32).map(|i| (i * 7 + 3) as u8).collect();
        let mut to = vec![0u8; from.len()];
        assert_eq!(guarded(to.as_mut_ptr(), from.as_ptr(), from.len()), 0);
        assert_eq!(to, from);
        // Nothing to copy is a copy.
        assert_eq!(guarded(to.as_mut_ptr(), from.as_ptr(), 0), 0);
    }

    /// [`copy_buffer`] has no [`LEAST`] floor: a buffer of a few bytes (a
    /// texture's last pixels, a record) is copied in place like any other.
    /// (Through `guarded`, as `copy_buffer` asks whether the copies are on.)
    #[test]
    fn a_buffer_of_a_few_bytes_is_copied() {
        let from: Vec<u8> = (0..64u32).map(|i| (i * 3 + 1) as u8).collect();
        for len in [1, 4, 16, 64] {
            let mut to = vec![0u8; len];
            assert!(guarded(to.as_mut_ptr(), from.as_ptr(), len));
            assert_eq!(to, from[..len]);
        }
    }

    /// Three pages reserved, the first committed: a copy that stays in the
    /// first page is whole; one that runs into the second ends with false,
    /// the bytes before the hole copied, and the thread goes on.
    #[test]
    fn a_page_that_is_not_there_ends_the_copy_without_a_fault() {
        // SAFETY: memory of the test's own.
        let base = unsafe { VirtualAlloc(std::ptr::null(), 3 * PAGE, MEM_RESERVE, PAGE_NOACCESS) } as *mut u8;
        assert!(!base.is_null());
        // SAFETY: the first page of that reservation.
        assert!(!unsafe { VirtualAlloc(base.cast(), PAGE, MEM_COMMIT, PAGE_READWRITE) }.is_null());
        // SAFETY: committed and writable.
        unsafe { std::slice::from_raw_parts_mut(base, PAGE) }.fill(0xAB);
        let mut to = vec![0u8; 2 * PAGE];
        assert_eq!(guarded(to.as_mut_ptr(), base, PAGE), 0);
        assert!(to[..PAGE].iter().all(|&b| b == 0xAB));
        to.fill(0);
        assert_eq!(guarded(to.as_mut_ptr(), base.wrapping_add(PAGE / 2), PAGE), PAGE / 2);
        assert!(to[..PAGE / 2].iter().all(|&b| b == 0xAB), "the bytes before the hole");
        assert!(to[PAGE / 2..].iter().all(|&b| b == 0), "nothing after it");
        // A source that is not there at all.
        assert_eq!(guarded(to.as_mut_ptr(), base.wrapping_add(PAGE), PAGE), PAGE);
        // Committed but closed to reads.
        // SAFETY: the third page of the reservation.
        assert!(!unsafe { VirtualAlloc(base.wrapping_add(2 * PAGE).cast(), PAGE, MEM_COMMIT, PAGE_NOACCESS) }.is_null());
        assert_eq!(guarded(to.as_mut_ptr(), base.wrapping_add(2 * PAGE), 16), 16);
        // And the registers the caller owns came back: this still works.
        assert_eq!(guarded(to.as_mut_ptr(), base, PAGE), 0);
        assert!(to[..PAGE].iter().all(|&b| b == 0xAB));
        // SAFETY: the test's reservation.
        unsafe { VirtualFree(base.cast(), 0, MEM_RELEASE) };
    }

    /// Small copies check just their blocks. Large copies can cover every
    /// cache slot, including one whose block number wrapped at the table size.
    #[test]
    fn recent_faults_match_only_copies_that_touch_the_block() {
        const BLOCK: usize = 64 << 10;
        let at = 2049 * BLOCK;
        let recent = RecentFaults::new();
        assert!(!recent.contains(at, 4096));
        recent.remember(at + 100);
        assert!(recent.contains(at, 16));
        assert!(recent.contains(at + BLOCK - 1, 1));
        assert!(recent.contains(at - 4096, 8192));
        assert!(recent.contains(0, at + 1));
        assert!(!recent.contains(0, at));
        assert!(!recent.contains(at + BLOCK, 4096));
        assert!(!recent.contains(at - BLOCK, 4096));
        assert!(!recent.contains(at - 1024 * BLOCK, 4096));
    }

    #[test]
    fn a_hole_between_readable_blocks_does_not_mark_the_ends() {
        const BLOCK: usize = 64 << 10;
        // SAFETY: the test reserves three blocks and commits its own end blocks.
        let base = unsafe { VirtualAlloc(std::ptr::null(), 3 * BLOCK, MEM_RESERVE, PAGE_NOACCESS) }
            .cast::<u8>();
        assert!(!base.is_null());
        assert_eq!(base as usize % BLOCK, 0);
        let end = base.wrapping_add(2 * BLOCK);
        for (address, byte) in [(base, 0xA5), (end, 0x5A)] {
            // SAFETY: the block is within the reservation and newly committed.
            unsafe {
                assert!(!VirtualAlloc(address.cast(), BLOCK, MEM_COMMIT, PAGE_READWRITE).is_null());
                address.write_bytes(byte, BLOCK);
            }
        }
        let recent = RecentFaults::new();
        let mut to = vec![0; 3 * BLOCK];
        let whole = copy_unless_recent_fault(to.as_mut_ptr(), base, to.len(), &recent);
        to.fill(0);
        let first = copy_unless_recent_fault(to.as_mut_ptr(), base, BLOCK, &recent);
        let last = copy_unless_recent_fault(to[2 * BLOCK..].as_mut_ptr(), end, BLOCK, &recent);
        let skipped_middle = recent.contains(base.wrapping_add(BLOCK) as usize, BLOCK);
        let skipped_whole = recent.contains(base as usize, to.len());
        // SAFETY: the test owns the reservation and is finished reading it.
        unsafe { VirtualFree(base.cast(), 0, MEM_RELEASE) };

        assert!(!whole);
        assert!(first, "an unrelated read of the first committed block must still use the fast path");
        assert!(last, "an unrelated read of the last committed block must still use the fast path");
        assert!(skipped_middle && skipped_whole);
        assert!(to[..BLOCK].iter().all(|&byte| byte == 0xA5));
        assert!(to[2 * BLOCK..].iter().all(|&byte| byte == 0x5A));
    }

    /// Many threads at once, half of them faulting: each gets its own answer.
    #[test]
    fn threads_fault_and_copy_side_by_side() {
        // SAFETY: memory of the test's own.
        let base = unsafe { VirtualAlloc(std::ptr::null(), 2 * PAGE, MEM_RESERVE, PAGE_NOACCESS) } as usize;
        assert_ne!(base, 0);
        // SAFETY: the first page of that reservation.
        assert!(!unsafe { VirtualAlloc(base as *const _, PAGE, MEM_COMMIT, PAGE_READWRITE) }.is_null());
        let threads: Vec<_> = (0..8)
            .map(|t| {
                std::thread::spawn(move || {
                    let mut to = vec![0u8; PAGE];
                    for _ in 0..2_000 {
                        let from = (base + if t % 2 == 0 { 0 } else { PAGE }) as *const u8;
                        let remaining = if t % 2 == 0 { 0 } else { PAGE };
                        assert_eq!(guarded(to.as_mut_ptr(), from, PAGE), remaining);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        // SAFETY: the test's reservation.
        unsafe { VirtualFree(base as *mut _, 0, MEM_RELEASE) };
    }

    #[test]
    fn a_readonly_or_unmapped_destination_ends_the_write_without_a_fault() {
        use windows_sys::Win32::System::Memory::PAGE_READONLY;
        // SAFETY: memory of the test's own.
        let base = unsafe { VirtualAlloc(std::ptr::null(), 3 * PAGE, MEM_RESERVE, PAGE_NOACCESS) } as *mut u8;
        assert!(!base.is_null());
        // SAFETY: page 0 writable, page 1 uncommitted, page 2 read-only.
        assert!(!unsafe { VirtualAlloc(base.cast(), PAGE, MEM_COMMIT, PAGE_READWRITE) }.is_null());
        assert!(!unsafe { VirtualAlloc(base.wrapping_add(2 * PAGE).cast(), PAGE, MEM_COMMIT, PAGE_READONLY) }.is_null());
        let src = vec![0x5Au8; 2 * PAGE];
        assert!(guarded_write(base, src.as_ptr(), PAGE));
        // SAFETY: committed and readable.
        assert!(unsafe { std::slice::from_raw_parts(base, PAGE) }.iter().all(|&b| b == 0x5A));
        // Spanning into uncommitted page 1 fails cleanly.
        assert!(!guarded_write(base.wrapping_add(PAGE / 2), src.as_ptr(), PAGE));
        // Writing to read-only page 2 fails cleanly.
        assert!(!guarded_write(base.wrapping_add(2 * PAGE), src.as_ptr(), 16));
        // Caller-saved registers intact: valid write still succeeds.
        assert!(guarded_write(base, src.as_ptr(), PAGE));
        // SAFETY: the test's reservation.
        unsafe { VirtualFree(base.cast(), 0, MEM_RELEASE) };
    }
}
