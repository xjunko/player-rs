pub fn format_duration_secs(secs: f64) -> String {
    let total = secs as u64;
    let mins = total / 60;
    let s = total % 60;
    format!("{}:{:02}", mins, s)
}
