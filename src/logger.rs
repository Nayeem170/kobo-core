pub const KLOG: &str = "/mnt/onboard/.adds/kothok.log";

pub struct FileLogger;
impl log::Log for FileLogger {
    fn log(&self, record: &log::Record) {
        use std::io::Write;
        let args = record.args().to_string();
        if args.contains("estimating duration") || args.contains("edge-tts WS connected") {
            return;
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| {
                let s = d.as_secs();
                format!(
                    "{:02}:{:02}:{:02}.{:03}",
                    (s / 3600) % 24,
                    (s / 60) % 60,
                    s % 60,
                    d.subsec_millis()
                )
            })
            .unwrap_or_else(|_| "??:??:??.???".into());
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(KLOG)
        {
            // best-effort: a failed log write has no recourse — cannot log a logging failure
            let _ = writeln!(f, "{} {:<5} {}", ts, record.level(), args);
            if record.level() <= log::Level::Info {
                // best-effort: sync failure means the log line may not survive a crash reboot
                let _ = f.sync_data();
            }
        }
    }
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn flush(&self) {}
}
