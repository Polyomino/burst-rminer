pub fn from_seconds(seconds: u64) -> String {
    let mut secs = seconds;
    let mut min = 0u64;
    let mut hour = 0u64;
    let mut day = 0u64;
    let mut year = 0u64;

    if secs > 59u64 {
        min = &secs / 60u64;
        secs = &secs % 60u64;
    }

    if min > 59u64 {
        hour = &min / 60u64;
        min = &min % 60u64;
    }

    if hour > 23u64 {
        day = &hour / 24u64;
        hour = &hour % 24u64;
    }

    if day > 364u64 {
        year = &day / 365u64;
        day = &day % 365u64;
    }

    return format!("{} years {} days {}:{:02}:{:02}",
                   year,
                   day,
                   hour,
                   min,
                   secs);
}