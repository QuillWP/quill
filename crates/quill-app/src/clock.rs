//! The local date and time, for the date and time fields. The standard
//! library has no time zones, so this asks the system directly.

/// Local date and time: year, month (1-12), day, hour (0-23), minute,
/// second and weekday (0 is Sunday).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub weekday: u32,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

#[cfg(unix)]
pub fn now() -> Now {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    if t == -1 || unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        // Date and time fields must remain harmless even if the operating
        // system cannot provide a clock value.
        return Now {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 4,
        };
    }
    Now {
        year: tm.tm_year + 1900,
        month: tm.tm_mon as u32 + 1,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        second: tm.tm_sec as u32,
        weekday: tm.tm_wday as u32,
    }
}

#[cfg(windows)]
pub fn now() -> Now {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(out: *mut SystemTime);
    }
    let mut st = SystemTime::default();
    unsafe { GetLocalTime(&mut st) };
    Now {
        year: st.year as i32,
        month: st.month as u32,
        day: st.day as u32,
        hour: st.hour as u32,
        minute: st.minute as u32,
        second: st.second as u32,
        weekday: st.day_of_week as u32,
    }
}

impl Now {
    /// A tracked change's date, to the minute, in local time as Word
    /// writes it: "2026-10-13T14:05:00Z".
    pub fn change_date(&self) -> String {
        format!(
            "{}T{:02}:{:02}:00Z",
            self.iso_date(),
            self.hour,
            self.minute
        )
    }

    /// The date as .docx keeps it: "2026-10-13".
    pub fn iso_date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// The original's short date: "10/13/26".
    pub fn date(&self) -> String {
        format!(
            "{}/{}/{:02}",
            self.month,
            self.day,
            self.year.rem_euclid(100)
        )
    }

    /// The original's time: "4:21 PM".
    pub fn time(&self) -> String {
        let h = match self.hour % 12 {
            0 => 12,
            h => h,
        };
        let ampm = if self.hour < 12 { "AM" } else { "PM" };
        format!("{h}:{:02} {ampm}", self.minute)
    }

    /// "4:21:07 PM".
    pub fn time_with_seconds(&self) -> String {
        let t = self.time();
        let (hm, ampm) = t.split_once(' ').unwrap_or((&t, ""));
        format!("{hm}:{:02} {ampm}", self.second)
    }

    pub fn month_name(&self) -> &'static str {
        MONTHS[(self.month as usize).clamp(1, 12) - 1]
    }

    pub fn day_name(&self) -> &'static str {
        DAYS[self.weekday as usize % 7]
    }

    /// "October 14, 2026", what Insert > Date types.
    pub fn date_normal(&self) -> String {
        format!("{} {}, {}", self.month_name(), self.day, self.year)
    }

    /// "Wed, Oct 14, 2026".
    pub fn date_abbreviated(&self) -> String {
        format!(
            "{}, {} {}, {}",
            &self.day_name()[..3],
            &self.month_name()[..3],
            self.day,
            self.year
        )
    }

    /// "Wednesday, October 14, 2026".
    pub fn date_long(&self) -> String {
        format!("{}, {}", self.day_name(), self.date_normal())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_word() {
        let n = Now {
            year: 2026,
            month: 10,
            day: 3,
            hour: 16,
            minute: 5,
            second: 9,
            weekday: 6,
        };
        assert_eq!(n.date(), "10/3/26");
        assert_eq!(n.date_normal(), "October 3, 2026");
        assert_eq!(n.date_abbreviated(), "Sat, Oct 3, 2026");
        assert_eq!(n.date_long(), "Saturday, October 3, 2026");
        assert_eq!(n.time_with_seconds(), "4:05:09 PM");
        assert_eq!(n.time(), "4:05 PM");
        let midnight = Now { hour: 0, ..n };
        assert_eq!(midnight.time(), "12:05 AM");
    }

    #[test]
    fn reads_the_system_clock() {
        let n = now();
        assert!(n.year >= 2024 && (1..=12).contains(&n.month) && (1..=31).contains(&n.day));
    }
}
