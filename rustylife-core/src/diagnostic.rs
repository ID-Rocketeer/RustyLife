use std::collections::HashSet;

fn main() {
    let rle = include_str!("patterns/breeder1.rle");
    let mut x = 0;
    let mut y = 0;
    let mut num: i128 = 0;
    let mut coords = HashSet::new();

    let data = rle.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('x'))
        .collect::<String>();

    let mut chars = data.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_digit(10) {
            num = num * 10 + ch.to_digit(10).unwrap() as i128;
        } else {
            let count = if num == 0 { 1 } else { num };
            num = 0;
            match ch {
                'b' => x += count,
                'o' => {
                    for i in 0..count {
                        coords.insert((x + i, y));
                    }
                    x += count;
                }
                '$' => {
                    y += count;
                    x = 0;
                }
                '!' => break,
                _ => {}
            }
        }
    }
    println!("Distinct cells: {}", coords.len());
}
