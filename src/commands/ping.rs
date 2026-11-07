use std::time::Instant;

use whatsapp_rust::bot::MessageContext;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Ping;

fn get_memory_stats() -> (String, String) {
    let mut rss_mb = 0.0;
    let mut heap_mb = 0.0;

    if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                if let Some(v) = line.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()) {
                    rss_mb = v / 1024.0;
                }
            } else if line.starts_with("RssAnon:") {
                if let Some(v) = line.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()) {
                    heap_mb = v / 1024.0;
                }
            }
        }
    }

    (format!("{rss_mb:.1} MB"), format!("{heap_mb:.1} MB"))
}

fn format_duration(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let rem_secs = secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}h"));
    }
    if hours > 0 {
        parts.push(format!("{hours}j"));
    }
    if mins > 0 {
        parts.push(format!("{mins}m"));
    }
    parts.push(format!("{rem_secs}d"));
    parts.join(" ")
}

impl Command for Ping {
    fn name(&self) -> &'static str {
        "ping"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["p", "speed", "status"]
    }

    fn description(&self) -> &'static str {
        "Cek responsivitas bot dan status server"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        _args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let start = Instant::now();

            let (rss, heap) = get_memory_stats();
            let uptime = format_duration(bot.start_time.elapsed().as_secs());
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

            let text = format!(
                "┌───『 ⚡ *SPEEDTEST & STATUS* 』───\n\
                 │ ⏱️ *Response:* {elapsed_ms:.2} ms\n\
                 │ 💾 *Memory:* {rss} (Heap {heap})\n\
                 │ ⏳ *Uptime:* {uptime}\n\
                 │ ⚙️ *Core:* Rust 1.94 (Tokio Async)\n\
                 │ 🌐 *Node:* Singapore VPS (melbi1)\n\
                 │ 🟢 *Status:* 100% Operational\n\
                 └───────────────────────────────"
            );

            ctx.reply_quoting(text).await?;
            Ok(())
        })
    }
}
