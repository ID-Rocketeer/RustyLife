pub fn fmt_num(val: i64, digits: usize, sign: bool) -> String {
    let s = format!("{:0width$}", val.abs(), width = digits);
    if sign {
        format!("{}{}", if val >= 0 { "+" } else { "-" }, s)
    } else {
        s
    }
}

pub fn format_si(val: f64, digits: usize, signed: bool) -> String {
    let units = ["", "K", "M", "G", "T"];
    // Sub-units: m (milli), u (micro), n (nano)
    let mut v = val.abs();

    // Handle 0 explicitly or very small numbers
    if v < f64::EPSILON {
        v = 0.0;
    }

    let s = if signed {
        if val >= 0.0 { "+" } else { "-" }
    } else {
        ""
    };

    let width = digits + 1 + 2; // e.g. 6 chars for "000.00"

    if v == 0.0 {
        return format!(
            "[ {}{} \u{00A0}/S ]",
            s,
            format!("{:0>width$.2}", 0.0, width = width)
        );
    }

    // Scale Up
    if v >= 1.0 {
        let mut u = 0;
        while v >= 999.995 && u < units.len() - 1 {
            v /= 1000.0;
            u += 1;
        }
        let n = format!("{:0>width$.2}", v, width = width);
        let unit = if units[u].is_empty() {
            "\u{00A0}"
        } else {
            units[u]
        };
        format!("[ {}{} {}/S ]", s, n, unit)
    } else {
        // Scale Down (m, u, n)
        let sub_units = ["m", "u", "n"];
        let mut su = 0;
        while v < 0.9995 && su < sub_units.len() {
            v *= 1000.0;
            su += 1;
        }

        let n = format!("{:0>width$.2}", v, width = width);
        let unit = if su > 0 {
            sub_units[su - 1]
        } else {
            "\u{00A0}"
        };
        format!("[ {}{} {}/S ]", s, n, unit)
    }
}
