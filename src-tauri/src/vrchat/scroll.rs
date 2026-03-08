use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use log::{error, info};

use super::format::{display_width, format_chatbox, MAX_INNER_WIDTH};
use super::osc::OscSender;

/// Shared state for the current chatbox message.
struct ChatboxMessage {
    original: String,
    translated: String,
    generation: u64,
}

/// Controls scrolling text display in VRChat chatbox.
/// Spawns a background thread that periodically sends OSC messages.
pub struct ScrollController {
    content: Arc<Mutex<Option<ChatboxMessage>>>,
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ScrollController {
    /// Create and start a new scroll controller.
    /// `pipeline_running` is the pipeline's shared running flag — the scroll thread
    /// will stop when this flag becomes false.
    pub fn new(port: u16, pipeline_running: Arc<AtomicBool>) -> Self {
        let content: Arc<Mutex<Option<ChatboxMessage>>> = Arc::new(Mutex::new(None));
        let running = Arc::new(AtomicBool::new(true));

        let content_clone = content.clone();
        let running_clone = running.clone();

        let handle = std::thread::Builder::new()
            .name("vrchat-scroll".into())
            .spawn(move || {
                scroll_thread(content_clone, running_clone, pipeline_running, port);
            })
            .expect("Failed to spawn vrchat-scroll thread");

        Self {
            content,
            running,
            handle: Some(handle),
        }
    }

    /// Update the current chatbox message. Resets scroll offsets.
    pub fn update(&self, original: &str, translated: &str, generation: u64) {
        if let Ok(mut lock) = self.content.lock() {
            *lock = Some(ChatboxMessage {
                original: original.to_string(),
                translated: translated.to_string(),
                generation,
            });
        }
    }

    /// Stop the scroll controller and join the thread.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ScrollController {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Background thread that sends OSC chatbox updates with scrolling.
fn scroll_thread(
    content: Arc<Mutex<Option<ChatboxMessage>>>,
    running: Arc<AtomicBool>,
    pipeline_running: Arc<AtomicBool>,
    port: u16,
) {
    let sender = match OscSender::new(port) {
        Ok(s) => s,
        Err(e) => {
            error!("VRChat OSC: failed to create sender: {e:#}");
            return;
        }
    };

    info!("VRChat OSC scroll thread started (port {port})");

    let mut last_send = Instant::now() - Duration::from_secs(10); // allow immediate first send
    let mut last_generation: u64 = 0;
    let mut offset_orig: usize = 0;
    let mut offset_trans: usize = 0;
    let mut orig_laps: usize = 0;
    let mut trans_laps: usize = 0;
    let mut done = false; // true when scrolling finished or static message sent

    let poll_interval = Duration::from_millis(50);
    let send_interval = Duration::from_millis(1500); // VRChat rate limit
    let scroll_step: usize = 3; // display columns per tick
    let max_laps: usize = 2; // stop scrolling after this many full cycles

    while running.load(Ordering::SeqCst) && pipeline_running.load(Ordering::SeqCst) {
        std::thread::sleep(poll_interval);

        let msg = match content.lock() {
            Ok(lock) => match lock.as_ref() {
                Some(m) => (m.original.clone(), m.translated.clone(), m.generation),
                None => continue,
            },
            Err(_) => continue,
        };

        let (original, translated, generation) = msg;

        // Reset scroll state on new message
        if generation != last_generation {
            last_generation = generation;
            offset_orig = 0;
            offset_trans = 0;
            orig_laps = 0;
            trans_laps = 0;
            done = false;
            // Force immediate send for new message
            last_send = Instant::now() - send_interval;
        }

        if done {
            continue;
        }

        // Check if scrolling is needed
        let orig_width = display_width(&original);
        let trans_width = display_width(&translated);
        let needs_scroll = orig_width > MAX_INNER_WIDTH || trans_width > MAX_INNER_WIDTH;

        // Respect rate limit
        if last_send.elapsed() < send_interval {
            continue;
        }

        // Extract visible windows
        let orig_window = extract_window(&original, offset_orig, MAX_INNER_WIDTH);
        let trans_window = extract_window(&translated, offset_trans, MAX_INNER_WIDTH);

        let formatted = format_chatbox(&orig_window, &trans_window, MAX_INNER_WIDTH);

        if let Err(e) = sender.send_chatbox(&formatted, true, false) {
            error!("VRChat OSC send error: {e:#}");
        }

        last_send = Instant::now();

        if needs_scroll {
            // Advance scroll offsets
            if orig_width > MAX_INNER_WIDTH {
                offset_orig += scroll_step;
                if offset_orig >= orig_width {
                    offset_orig = 0;
                    orig_laps += 1;
                }
            }
            if trans_width > MAX_INNER_WIDTH {
                offset_trans += scroll_step;
                if offset_trans >= trans_width {
                    offset_trans = 0;
                    trans_laps += 1;
                }
            }
            // Stop after both lines have completed their laps
            let orig_done = orig_width <= MAX_INNER_WIDTH || orig_laps >= max_laps;
            let trans_done = trans_width <= MAX_INNER_WIDTH || trans_laps >= max_laps;
            if orig_done && trans_done {
                done = true;
            }
        } else {
            // Static message — sent once, VRChat will expire it on its own
            done = true;
        }
    }

    // Clear chatbox on stop
    let _ = sender.send_chatbox("", true, false);
    info!("VRChat OSC scroll thread stopped");
}

/// Extract a visible substring window starting at `offset` display columns,
/// wrapping around if needed, with a maximum of `max_width` display columns.
fn extract_window(s: &str, offset: usize, max_width: usize) -> String {
    let total_width = display_width(s);
    if total_width <= max_width {
        return s.to_string();
    }

    // Collect chars with their display widths
    let chars: Vec<(char, usize)> = s
        .chars()
        .map(|c| (c, unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)))
        .collect();

    let effective_offset = offset % total_width;
    let mut result = String::new();
    let mut current_width = 0;
    let mut col = 0;
    let mut idx = 0;

    // Skip to offset
    for (i, &(_, w)) in chars.iter().enumerate() {
        if col + w > effective_offset {
            idx = i;
            break;
        }
        col += w;
        if i == chars.len() - 1 {
            idx = 0;
            col = 0;
        }
    }

    // Collect characters from offset
    let len = chars.len();
    let mut pos = idx;
    while current_width < max_width {
        let (ch, w) = chars[pos % len];
        if current_width + w > max_width {
            break;
        }
        result.push(ch);
        current_width += w;
        pos += 1;
        if pos >= len {
            pos = 0;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_window_no_scroll() {
        assert_eq!(extract_window("hello", 0, 31), "hello");
    }

    #[test]
    fn test_extract_window_with_offset() {
        let s = "abcdefghijklmnopqrstuvwxyz0123456789";
        let window = extract_window(s, 5, 10);
        assert_eq!(window, "fghijklmno");
    }

    #[test]
    fn test_extract_window_wraps() {
        let s = "abcdefghij"; // 10 chars
        let window = extract_window(s, 8, 5);
        assert_eq!(window, "ijabc");
    }
}
