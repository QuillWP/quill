//! List numbers in the number formats Word names (`w:numFmt`), for the
//! list numbers Quill turns into text when it opens a file.

/// Number `n` written in Word's format `fmt`. `custom` is a custom
/// format's pattern (`w:numFmt w:format`), like "001, 002, 003, ...".
/// Formats Quill does not know are written as plain decimal numbers.
pub(crate) fn format_number(n: i32, fmt: &str, custom: Option<&str>) -> String {
    match fmt {
        "lowerLetter" => letters(n, false),
        "upperLetter" => letters(n, true),
        "lowerRoman" => roman(n).to_lowercase(),
        "upperRoman" => roman(n),
        // 01, 02 ... 09, then 10 on as usual.
        "decimalZero" if (0..10).contains(&n) => format!("0{n}"),
        "ordinal" => {
            let suffix = match (n % 10, n % 100) {
                (_, 11..=13) => "th",
                (1, _) => "st",
                (2, _) => "nd",
                (3, _) => "rd",
                _ => "th",
            };
            format!("{n}{suffix}")
        }
        "cardinalText" => capitalize(&english(n, false)),
        "ordinalText" => capitalize(&english(n, true)),
        "custom" => custom_pattern(n, custom.unwrap_or_default()),
        "decimalFullWidth" | "decimalFullWidth2" => n
            .to_string()
            .chars()
            .map(|c| match c.to_digit(10) {
                Some(d) => char::from_u32(0xFF10 + d).unwrap_or(c),
                None => c,
            })
            .collect(),
        "decimalEnclosedCircle" | "decimalEnclosedCircleChinese" if (1..=20).contains(&n) => {
            char::from_u32(0x2460 + n as u32 - 1).map_or(n.to_string(), String::from)
        }
        "decimalEnclosedFullstop" if (1..=20).contains(&n) => {
            char::from_u32(0x2488 + n as u32 - 1).map_or(n.to_string(), String::from)
        }
        "decimalEnclosedParen" if (1..=20).contains(&n) => {
            char::from_u32(0x2474 + n as u32 - 1).map_or(n.to_string(), String::from)
        }
        // Word goes on in plain numbers after the ten stems and the twelve
        // signs.
        "ideographTraditional" => one_of(n, "甲乙丙丁戊己庚辛壬癸"),
        "ideographZodiac" => one_of(n, "子丑寅卯辰巳午未申酉戌亥"),
        "ideographDigital" => digits(n, "〇一二三四五六七八九"),
        "koreanDigital" => digits(n, "영일이삼사오육칠팔구"),
        "koreanDigital2" => digits(n, "零一二三四五六七八九"),
        "japaneseCounting" | "koreanCounting" => counting(n, "一二三四五六七八九", "十百千", false),
        "chineseCounting"
        | "chineseCountingThousand"
        | "taiwaneseCounting"
        | "taiwaneseCountingThousand" => counting(n, "一二三四五六七八九", "十百千", true),
        "ideographLegalTraditional" => legal(n, "壹貳參肆伍陸柒捌玖", "拾佰仟"),
        "chineseLegalSimplified" => legal(n, "壹贰叁肆伍陆柒捌玖", "拾佰仟"),
        "japaneseLegal" => legal(n, "壱弐参四伍六七八九", "拾百阡"),
        "aiueo" => cycle(n, AIUEO_HALF),
        "aiueoFullWidth" => cycle(n, AIUEO),
        "iroha" => cycle(n, IROHA_HALF),
        "irohaFullWidth" => cycle(n, IROHA),
        "ganada" => cycle(n, "가나다라마바사아자차카타파하"),
        "chosung" => cycle(n, "ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ"),
        "russianLower" => cycle(n, RUSSIAN),
        "russianUpper" => cycle(n, &RUSSIAN.to_uppercase()),
        "arabicAlpha" => cycle(n, "أبتثجحخدذرزسشصضطظعغفقكلمنهوي"),
        "thaiLetters" => cycle(n, "กขคงจฉชซฌญฎฏฐฑฒณดตถทธนบปผฝพฟภมยรลวศษสหฬอฮ"),
        "hebrew1" => hebrew(n),
        "chicago" => chicago(n),
        _ => n.to_string(),
    }
}

const AIUEO: &str =
    "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン";
const AIUEO_HALF: &str = "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜｦﾝ";
const IROHA: &str = "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス";
const IROHA_HALF: &str = "ｲﾛﾊﾆﾎﾍﾄﾁﾘﾇﾙｦﾜｶﾖﾀﾚｿﾂﾈﾅﾗﾑｳヰﾉｵｸﾔﾏｹﾌｺｴﾃｱｻｷﾕﾒﾐｼヱﾋﾓｾｽ";
const RUSSIAN: &str = "абвгдежзиклмнопрстуфхцчшщэюя";

/// Most times a letter or mark is repeated. Word writes as many as the
/// number needs; a huge start value must not make a huge string.
const MAX_REPEAT: usize = 10;

/// a, b ... z, then aa, bb ... as Word does, up to ten of a letter.
pub(crate) fn letters(n: i32, upper: bool) -> String {
    if n < 1 {
        return n.to_string();
    }
    let base = if upper { b'A' } else { b'a' };
    let c = (base + ((n - 1) % 26) as u8) as char;
    c.to_string()
        .repeat((((n - 1) / 26 + 1) as usize).min(MAX_REPEAT))
}

pub fn roman(mut n: i32) -> String {
    if !(1..=3999).contains(&n) {
        return n.to_string();
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in table {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// The `n`th of `set`, going round again after the last.
fn cycle(n: i32, set: &str) -> String {
    let set: Vec<char> = set.chars().collect();
    if n < 1 || set.is_empty() {
        return n.to_string();
    }
    set[(n as usize - 1) % set.len()].to_string()
}

/// The `n`th of `set`, or a plain number past its end.
fn one_of(n: i32, set: &str) -> String {
    match set.chars().nth((n as usize).wrapping_sub(1)) {
        Some(c) if n >= 1 => c.to_string(),
        _ => n.to_string(),
    }
}

/// Each decimal digit of `n` as the matching character of `set` (0 to 9).
fn digits(n: i32, set: &str) -> String {
    let set: Vec<char> = set.chars().collect();
    n.to_string()
        .chars()
        .map(|c| c.to_digit(10).map_or(c, |d| set[d as usize]))
        .collect()
}

/// CJK counting numerals below 10,000: 十一 for 11, 二十 for 20. Chinese
/// writes a zero (〇) where a place is skipped inside a number (一百〇一)
/// and the one before a hundred or thousand, and before ten in numbers
/// above a hundred (一百一十); Japanese does neither (百一, 百十).
fn counting(n: i32, ones: &str, places: &str, chinese: bool) -> String {
    let ones: Vec<char> = ones.chars().collect();
    let places: Vec<char> = places.chars().collect();
    if !(1..10_000).contains(&n) {
        return n.to_string();
    }
    let ds = [n / 1000, n / 100 % 10, n / 10 % 10, n % 10];
    let mut out = String::new();
    let mut gap = false;
    for (k, &d) in ds.iter().enumerate() {
        if d == 0 {
            gap = !out.is_empty();
            continue;
        }
        if gap && chinese {
            out.push('〇');
        }
        gap = false;
        let place = 3 - k;
        // Chinese writes the one before a hundred or thousand always, and
        // before ten inside bigger numbers; Japanese never does.
        let write_one = place == 0 || (chinese && (place >= 2 || n >= 100));
        if d != 1 || write_one {
            out.push(ones[d as usize - 1]);
        }
        if place > 0 {
            out.push(places[place - 1]);
        }
    }
    out
}

/// Legal (financial) CJK numerals: every place keeps its digit, so 10 is
/// 壹拾 and 11 is 壹拾壹.
fn legal(n: i32, ones: &str, places: &str) -> String {
    let ones: Vec<char> = ones.chars().collect();
    let places: Vec<char> = places.chars().collect();
    if !(1..10_000).contains(&n) {
        return n.to_string();
    }
    let ds = [n / 1000, n / 100 % 10, n / 10 % 10, n % 10];
    let mut out = String::new();
    for (k, &d) in ds.iter().enumerate() {
        if d == 0 {
            continue;
        }
        out.push(ones[d as usize - 1]);
        let place = 3 - k;
        if place > 0 {
            out.push(places[place - 1]);
        }
    }
    out
}

/// Hebrew letters as numbers: 1 to 9, the tens and the hundreds, with
/// 15 and 16 written 9+6 and 9+7 as Hebrew does.
fn hebrew(n: i32) -> String {
    if !(1..1000).contains(&n) {
        return n.to_string();
    }
    let ones = ['א', 'ב', 'ג', 'ד', 'ה', 'ו', 'ז', 'ח', 'ט'];
    let tens = ['י', 'כ', 'ל', 'מ', 'נ', 'ס', 'ע', 'פ', 'צ'];
    let hundreds = ['ק', 'ר', 'ש', 'ת'];
    let mut out = String::new();
    let mut h = n / 100;
    while h > 0 {
        let k = h.min(4);
        out.push(hundreds[k as usize - 1]);
        h -= k;
    }
    let rest = n % 100;
    match rest {
        15 => out.push_str("טו"),
        16 => out.push_str("טז"),
        _ => {
            if rest >= 10 {
                out.push(tens[(rest / 10) as usize - 1]);
            }
            if rest % 10 > 0 {
                out.push(ones[(rest % 10) as usize - 1]);
            }
        }
    }
    out
}

/// *, †, ‡, §, then each doubled, tripled and so on.
fn chicago(n: i32) -> String {
    if n < 1 {
        return n.to_string();
    }
    let marks = ['*', '†', '‡', '§'];
    marks[(n as usize - 1) % 4]
        .to_string()
        .repeat(((n as usize - 1) / 4 + 1).min(MAX_REPEAT))
}

/// A custom format's pattern: "001, 002, 003, ..." pads to its first
/// number's width; anything else is written as a plain number.
fn custom_pattern(n: i32, pattern: &str) -> String {
    let first = pattern.split(',').next().unwrap_or_default().trim();
    if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) && first.ends_with('1') {
        format!("{n:0width$}", width = first.len())
    } else {
        n.to_string()
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// `n` in English words, as a count ("twenty-one") or an ordinal
/// ("twenty-first").
fn english(n: i32, ordinal: bool) -> String {
    const ONES: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 10] = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    fn below_thousand(n: i32) -> String {
        let mut parts = Vec::new();
        if n >= 100 {
            parts.push(format!("{} hundred", ONES[(n / 100) as usize]));
        }
        let rest = n % 100;
        if rest > 0 {
            let words = if rest < 20 {
                ONES[rest as usize].to_string()
            } else if rest % 10 == 0 {
                TENS[(rest / 10) as usize].to_string()
            } else {
                format!(
                    "{}-{}",
                    TENS[(rest / 10) as usize],
                    ONES[(rest % 10) as usize]
                )
            };
            parts.push(words);
        }
        parts.join(" ")
    }
    if !(0..1_000_000).contains(&n) {
        return n.to_string();
    }
    let words = if n == 0 {
        "zero".to_string()
    } else {
        let mut parts = Vec::new();
        if n >= 1000 {
            parts.push(format!("{} thousand", below_thousand(n / 1000)));
        }
        if n % 1000 > 0 {
            parts.push(below_thousand(n % 1000));
        }
        parts.join(" ")
    };
    if !ordinal {
        return words;
    }
    // Only the last word changes: twenty-one -> twenty-first.
    let cut = words.rfind([' ', '-']).map_or(0, |i| i + 1);
    let (head, last) = words.split_at(cut);
    let last = match last {
        "one" => "first".to_string(),
        "two" => "second".to_string(),
        "three" => "third".to_string(),
        "five" => "fifth".to_string(),
        "eight" => "eighth".to_string(),
        "nine" => "ninth".to_string(),
        "twelve" => "twelfth".to_string(),
        w if w.ends_with('y') => format!("{}ieth", &w[..w.len() - 1]),
        w => format!("{w}th"),
    };
    format!("{head}{last}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_letters_and_marks_stop_growing() {
        assert_eq!(letters(1_000_000, false).len(), 10);
        assert_eq!(chicago(1_000_000).chars().count(), 10);
        assert_eq!(letters(27, false), "aa");
    }

    #[test]
    fn words() {
        let f = |n, fmt| format_number(n, fmt, None);
        assert_eq!(f(1, "cardinalText"), "One");
        assert_eq!(f(20, "cardinalText"), "Twenty");
        assert_eq!(f(21, "cardinalText"), "Twenty-one");
        assert_eq!(f(115, "cardinalText"), "One hundred fifteen");
        assert_eq!(f(1, "ordinalText"), "First");
        assert_eq!(f(12, "ordinalText"), "Twelfth");
        assert_eq!(f(20, "ordinalText"), "Twentieth");
        assert_eq!(f(23, "ordinalText"), "Twenty-third");
        assert_eq!(f(100, "ordinalText"), "One hundredth");
    }

    #[test]
    fn east_asian() {
        let f = |n, fmt| format_number(n, fmt, None);
        assert_eq!(f(1, "ideographTraditional"), "甲");
        assert_eq!(f(10, "ideographTraditional"), "癸");
        assert_eq!(f(11, "ideographTraditional"), "11");
        assert_eq!(f(13, "ideographZodiac"), "13");
        assert_eq!(f(3, "ideographZodiac"), "寅");
        assert_eq!(f(10, "ideographLegalTraditional"), "壹拾");
        assert_eq!(f(12, "ideographLegalTraditional"), "壹拾貳");
        assert_eq!(f(20, "chineseLegalSimplified"), "贰拾");
        assert_eq!(f(3, "japaneseLegal"), "参");
        assert_eq!(f(10, "japaneseCounting"), "十");
        assert_eq!(f(11, "japaneseCounting"), "十一");
        assert_eq!(f(20, "japaneseCounting"), "二十");
        assert_eq!(f(101, "japaneseCounting"), "百一");
        assert_eq!(f(101, "chineseCounting"), "一百〇一");
        assert_eq!(f(110, "chineseCounting"), "一百一十");
        assert_eq!(f(1, "irohaFullWidth"), "イ");
        assert_eq!(f(2, "iroha"), "ﾛ");
        assert_eq!(f(15, "ganada"), "가");
        assert_eq!(f(3, "decimalEnclosedCircle"), "③");
        assert_eq!(f(12, "decimalFullWidth"), "１２");
    }

    #[test]
    fn others() {
        let f = |n, fmt| format_number(n, fmt, None);
        assert_eq!(f(15, "hebrew1"), "טו");
        assert_eq!(f(123, "hebrew1"), "קכג");
        assert_eq!(f(5, "chicago"), "**");
        assert_eq!(
            format_number(7, "custom", Some("001, 002, 003, ...")),
            "007"
        );
        assert_eq!(f(7, "somethingNew"), "7");
    }
}
