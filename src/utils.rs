use lopdf::{Dictionary, Object};

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
