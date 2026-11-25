//! Diagnostics (`VICTORY_WATER_LOOK=1`): what the title's water height
//! look-ups give back.
//!
//! `ShallowWaterSim:CS_ResolveHeightLookUps` answers the physics' questions
//! for the water's height on the GPU (32-byte records in, one float each
//! out; -10000 is "no water here"). Boats that sink as physics takes over
//! (fxceless/victory-fork#863) may get no answer, a late one, or "no
//! water". Here the buffers it is bound to are noted, the title's copies
//! out of what it writes are noted too, and a thread looks at title memory
//! ten times a second and says what is there when it changes. Nothing is
//! written.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

/// The program, by the hash its pipeline is logged with.
const PROGRAM: &str = "92b4790eb4c29f6d";
/// Bytes looked at of a buffer, and buffers kept.
const MOST: u64 = 64 << 10;
const KEPT: usize = 48;

static BOUND: AtomicU64 = AtomicU64::new(0);
static COPIED: AtomicU64 = AtomicU64::new(0);

/// Where title memory is looked at: address to bytes and what it is.
fn watched() -> std::sync::MutexGuard<'static, HashMap<u64, (u64, String)>> {
    static WATCHED: OnceLock<Mutex<HashMap<u64, (u64, String)>>> = OnceLock::new();
    WATCHED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var_os("VICTORY_WATER_LOOK").is_some_and(|v| v == "1");
        if on {
            tracing::info!("water height look-ups are watched (VICTORY_WATER_LOOK=1)");
            std::thread::spawn(look);
        }
        on
    })
}

pub(crate) fn is(name: &str) -> bool {
    name.contains(PROGRAM)
}

/// The program is bound for a dispatch, to `views` (address, bytes, what).
pub(crate) fn bound(views: Vec<(u64, u64, String)>) {
    let n = BOUND.fetch_add(1, Relaxed);
    let mut watched = watched();
    for (address, bytes, what) in views {
        if bytes == 0 || watched.contains_key(&address) || watched.len() >= KEPT {
            continue;
        }
        tracing::info!(n, address = format_args!("{address:#x}"), bytes, what, "water look-ups: a buffer");
        watched.insert(address, (bytes, what));
    }
}

/// A copy of title memory is recorded. One out of what the program writes
/// is the read-back: its destination is watched too.
pub(crate) fn copied(dst: u64, src: u64, len: u64) {
    let mut watched = watched();
    let from = watched
        .iter()
        .any(|(&a, (n, what))| what.starts_with("written") && src < a + *n && a < src + len);
    if !from {
        return;
    }
    let n = COPIED.fetch_add(1, Relaxed);
    if !watched.contains_key(&dst) && watched.len() < KEPT {
        tracing::info!(
            n,
            bound = BOUND.load(Relaxed),
            dst = format_args!("{dst:#x}"),
            src = format_args!("{src:#x}"),
            len,
            "water look-ups: the title copies them out"
        );
        watched.insert(dst, (len, "copied out".to_owned()));
    }
}

/// What `bytes` hold, as floats.
fn said(bytes: &[u8]) -> String {
    let values: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let none = values.iter().filter(|&&v| v <= -9999.0).count();
    let other = values.iter().filter(|v| !v.is_finite()).count();
    let real: Vec<f32> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > -9999.0 && v.abs() > 1e-6)
        .collect();
    let (low, high) = real.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    let first = values.iter().take(16).map(|v| format!("{v:.3}")).collect::<Vec<_>>().join(" ");
    format!(
        "{} values: {none} no water, {other} not numbers, {} others than zero ({low:.3} to {high:.3}); first {first}",
        values.len(),
        real.len()
    )
}

fn look() {
    let mut last: HashMap<u64, (Vec<u8>, u64)> = HashMap::new();
    let start = std::time::Instant::now();
    let mut line = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let now = start.elapsed().as_millis() as u64;
        let all: Vec<(u64, u64, String)> =
            watched().iter().map(|(&a, (n, what))| (a, *n, what.clone())).collect();
        for (address, n, what) in all {
            let mut bytes = vec![0; n.min(MOST) as usize];
            if !crate::host::read_into(address as *const u8, &mut bytes) {
                continue;
            }
            // At most one line a second for a buffer.
            if last.get(&address).is_some_and(|(b, at)| *b == bytes || now < at + 1000) {
                continue;
            }
            tracing::info!(
                address = format_args!("{address:#x}"),
                frame = crate::frame::current(),
                bound = BOUND.load(Relaxed),
                copied = COPIED.load(Relaxed),
                "water look-ups, {what}: {}",
                said(&bytes)
            );
            last.insert(address, (bytes, now));
        }
        line += 1;
        if line.is_multiple_of(50) {
            tracing::info!(
                frame = crate::frame::current(),
                bound = BOUND.load(Relaxed),
                copied = COPIED.load(Relaxed),
                "water look-ups so far"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn no_water_is_told_from_heights() {
        let bytes: Vec<u8> = [-10000.0f32, 0.0, 2.5, f32::NAN]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let text = super::said(&bytes);
        assert!(
            text.starts_with("4 values: 1 no water, 1 not numbers, 1 others than zero (2.500 to 2.500)"),
            "{text}"
        );
    }
}
