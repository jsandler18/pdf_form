use lopdf::{Dictionary, Object};

use crate::from_utf8;

bitflags! {
    pub struct FieldFlags: u32 {
        const READONLY          = 0x1;
        const REQUIRED          = 0x2;
    }
}

bitflags! {
    pub struct ButtonFlags: u32 {
        const NO_TOGGLE_TO_OFF  = 0x8000;
        const RADIO             = 0x10000;
        const PUSHBUTTON        = 0x20000;
        const RADIO_IN_UNISON   = 0x4000000;

    }
}

bitflags! {
    pub struct ChoiceFlags: u32 {
        const COBMO             = 0x20000;
        const EDIT              = 0x40000;
        const SORT              = 0x80000;
        const MULTISELECT       = 0x200000;
        const DO_NOT_SPELLCHECK = 0x800000;
        const COMMIT_ON_CHANGE  = 0x8000000;
    }
}

pub fn is_read_only(field: &Dictionary) -> bool {
    let flags = FieldFlags::from_bits_truncate(get_field_flags(field));

    flags.intersects(FieldFlags::READONLY)
}

pub fn is_required(field: &Dictionary) -> bool {
    let flags = FieldFlags::from_bits_truncate(get_field_flags(field));

    flags.intersects(FieldFlags::REQUIRED)
}

pub fn get_field_flags(field: &Dictionary) -> u32 {
    field
        .get(b"Ff")
        .unwrap_or(&Object::Integer(0))
        .as_i64()
        .unwrap() as u32
}

pub fn get_on_value(field: &Dictionary) -> String {
    let mut option = None;
    if let Ok(ap) = field.get(b"AP") {
        if let Ok(dict) = ap.as_dict() {
            if let Ok(values) = dict.get(b"N") {
                if let Ok(options) = values.as_dict() {
                    for (name, _) in options {
                        if let Ok(name) = from_utf8(name) {
                            if name != "Off" && option.is_none() {
                                option = Some(name.into());
                            }
                        }
                    }
                }
            }
        }
    }

    option.unwrap_or("Yes".into())
}

pub fn parse_font(font_string: Option<&str>) -> ((&str, i32), (&str, i32, i32, i32, i32)) {
    // The default font object (/Helv 12 Tf 0 g)
    let default_font = ("Helv", 12);
    let default_color = ("g", 0, 0, 0, 0);

    // Build the font basing on the default appearance, if exists, if not,
    // assume a default font (surely to be improved!)
    match font_string {
        Some(font_string) => {
            let font = font_string
                .trim_start_matches('/')
                .split("Tf")
                .collect::<Vec<_>>();

            if font.len() < 2 {
                (default_font, default_color)
            } else {
                let font_family = font[0].trim().split(' ').collect::<Vec<_>>();
                let font_color = font[1].trim().split(' ').collect::<Vec<_>>();

                let font = if font_family.len() >= 2 {
                    (font_family[0], font_family[1].parse::<i32>().unwrap_or(0))
                } else {
                    default_font
                };

                let color = if font_color.len() == 2 {
                    ("g", font_color[0].parse::<i32>().unwrap_or(0), 0, 0, 0)
                } else if font_color.len() == 4 {
                    (
                        "rg",
                        font_color[0].parse::<i32>().unwrap_or(0),
                        font_color[1].parse::<i32>().unwrap_or(0),
                        font_color[2].parse::<i32>().unwrap_or(0),
                        0,
                    )
                } else if font_color.len() == 5 {
                    (
                        "k",
                        font_color[0].parse::<i32>().unwrap_or(0),
                        font_color[1].parse::<i32>().unwrap_or(0),
                        font_color[2].parse::<i32>().unwrap_or(0),
                        font_color[3].parse::<i32>().unwrap_or(0),
                    )
                } else {
                    default_color
                };

                (font, color)
            }
        }
        _ => (default_font, default_color),
    }
}
