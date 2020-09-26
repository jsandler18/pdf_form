#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate derive_error;

mod utils;

use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::path::Path;
use std::str;

use bitflags::_core::str::from_utf8;

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, ObjectId, StringFormat};

use crate::utils::*;

/// A PDF Form that contains fillable fields
///
/// Use this struct to load an existing PDF with a fillable form using the `load` method.  It will
/// analyze the PDF and identify the fields. Then you can get and set the content of the fields by
/// index.
pub struct Form {
    doc: Document,
    form_ids: Vec<ObjectId>,
}

/// The possible types of fillable form fields in a PDF
#[derive(Debug)]
pub enum FieldType {
    Button,
    Radio,
    CheckBox,
    ListBox,
    ComboBox,
    Text,
    Unknown,
}

#[derive(Debug, Error)]
/// Errors that may occur while loading a PDF
pub enum LoadError {
    /// An Lopdf Error
    LopdfError(lopdf::Error),
    /// The reference `ObjectId` did not point to any values
    #[error(non_std, no_from)]
    NoSuchReference(ObjectId),
    /// An element that was expected to be a reference was not a reference
    NotAReference,
}

/// Errors That may occur while setting values in a form
#[derive(Debug, Error)]
pub enum ValueError {
    /// The method used to set the state is incompatible with the type of the field
    TypeMismatch,
    /// One or more selected values are not valid choices
    InvalidSelection,
    /// Multiple values were selected when only one was allowed
    TooManySelected,
    /// Readonly field cannot be edited
    Readonly,
}
/// The current state of a form field
#[derive(Debug)]
pub enum FieldState {
    /// Push buttons have no state
    Button,
    /// `selected` is the singular option from `options` that is selected
    Radio {
        selected: String,
        options: Vec<String>,
        readonly: bool,
        required: bool,
    },
    /// The toggle state of the checkbox
    CheckBox {
        is_checked: bool,
        readonly: bool,
        required: bool,
    },
    /// `selected` is the list of selected options from `options`
    ListBox {
        selected: Vec<String>,
        options: Vec<String>,
        multiselect: bool,
        readonly: bool,
        required: bool,
    },
    /// `selected` is the list of selected options from `options`
    ComboBox {
        selected: Vec<String>,
        options: Vec<String>,
        editable: bool,
        readonly: bool,
        required: bool,
    },
    /// User Text Input
    Text {
        text: String,
        readonly: bool,
        required: bool,
    },
    /// Unknown fields have no state
    Unknown,
}

trait PdfObjectDeref {
    fn deref<'a>(&self, doc: &'a Document) -> Result<&'a Object, LoadError>;
}

impl PdfObjectDeref for Object {
    fn deref<'a>(&self, doc: &'a Document) -> Result<&'a Object, LoadError> {
        match *self {
            Object::Reference(oid) => doc.objects.get(&oid).ok_or(LoadError::NoSuchReference(oid)),
            _ => Err(LoadError::NotAReference),
        }
    }
}

impl Form {
    /// Takes a reader containing a PDF with a fillable form, analyzes the content, and attempts to
    /// identify all of the fields the form has.
    pub fn load_from<R: io::Read>(reader: R) -> Result<Self, LoadError> {
        let doc = Document::load_from(reader)?;
        Self::load_doc(doc)
    }

    /// Takes a path to a PDF with a fillable form, analyzes the file, and attempts to identify all
    /// of the fields the form has.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, LoadError> {
        let doc = Document::load(path)?;
        Self::load_doc(doc)
    }

    fn load_doc(mut doc: Document) -> Result<Self, LoadError> {
        let mut form_ids = Vec::new();
        let mut queue = VecDeque::new();
        // Block so borrow of doc ends before doc is moved into the result
        {
            doc.decompress();

            let acroform = doc
                .objects
                .get_mut(
                    &doc.trailer
                        .get(b"Root")?
                        .deref(&doc)?
                        .as_dict()?
                        .get(b"AcroForm")?
                        .as_reference()?,
                )
                .ok_or(LoadError::NotAReference)?
                .as_dict_mut()?;

            // Sets the NeedAppearances option to true into "AcroForm" in order
            // to render fields correctly
            // acroform.set("NeedAppearances", Object::Boolean(true));

            let fields_list = acroform.get(b"Fields")?.as_array()?;
            queue.append(&mut VecDeque::from(fields_list.clone()));

            // Iterate over the fields
            while let Some(objref) = queue.pop_front() {
                let obj = objref.deref(&doc)?;
                if let Object::Dictionary(ref dict) = *obj {
                    // If the field has FT, it actually takes input.  Save this
                    if dict.get(b"FT").is_ok() {
                        form_ids.push(objref.as_reference().unwrap());
                    }

                    // If this field has kids, they might have FT, so add them to the queue
                    if let Ok(&Object::Array(ref kids)) = dict.get(b"Kids") {
                        queue.append(&mut VecDeque::from(kids.clone()));
                    }
                }
            }
        }
        Ok(Form { doc, form_ids })
    }

    /// Returns the number of fields the form has
    pub fn len(&self) -> usize {
        self.form_ids.len()
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Gets the type of field of the given index
    ///
    /// # Panics
    /// This function will panic if the index is greater than the number of fields
    pub fn get_type(&self, n: usize) -> FieldType {
        // unwraps should be fine because load should have verified everything exists
        let field = self
            .doc
            .objects
            .get(&self.form_ids[n])
            .unwrap()
            .as_dict()
            .unwrap();

        let type_str = field.get(b"FT").unwrap().as_name_str().unwrap();
        if type_str == "Btn" {
            let flags = ButtonFlags::from_bits_truncate(get_field_flags(field));
            if flags.intersects(ButtonFlags::RADIO | ButtonFlags::NO_TOGGLE_TO_OFF) {
                FieldType::Radio
            } else if flags.intersects(ButtonFlags::PUSHBUTTON) {
                FieldType::Button
            } else {
                FieldType::CheckBox
            }
        } else if type_str == "Ch" {
            let flags = ChoiceFlags::from_bits_truncate(get_field_flags(field));
            if flags.intersects(ChoiceFlags::COBMO) {
                FieldType::ComboBox
            } else {
                FieldType::ListBox
            }
        } else if type_str == "Tx" {
            FieldType::Text
        } else {
            FieldType::Unknown
        }
    }

    /// Gets the name of field of the given index
    ///
    /// # Panics
    /// This function will panic if the index is greater than the number of fields
    pub fn get_name(&self, n: usize) -> Option<String> {
        // unwraps should be fine because load should have verified everything exists
        let field = self
            .doc
            .objects
            .get(&self.form_ids[n])
            .unwrap()
            .as_dict()
            .unwrap();

        // The "T" key refers to the name of the field
        match field.get(b"T") {
            Ok(Object::String(data, _)) => String::from_utf8(data.clone()).ok(),
            _ => None,
        }
    }

    /// Gets the types of all of the fields in the form
    pub fn get_all_types(&self) -> Vec<FieldType> {
        let mut res = Vec::with_capacity(self.len());
        for i in 0..self.len() {
            res.push(self.get_type(i))
        }
        res
    }

    /// Gets the names of all of the fields in the form
    pub fn get_all_names(&self) -> Vec<Option<String>> {
        let mut res = Vec::with_capacity(self.len());
        for i in 0..self.len() {
            res.push(self.get_name(i))
        }
        res
    }

    /// Gets the state of field of the given index
    ///
    /// # Panics
    /// This function will panic if the index is greater than the number of fields
    pub fn get_state(&self, n: usize) -> FieldState {
        let field = self
            .doc
            .objects
            .get(&self.form_ids[n])
            .unwrap()
            .as_dict()
            .unwrap();
        match self.get_type(n) {
            FieldType::Button => FieldState::Button,
            FieldType::Radio => FieldState::Radio {
                selected: match field.get(b"V") {
                    Ok(name) => name.as_name_str().unwrap().to_owned(),
                    _ => match field.get(b"AS") {
                        Ok(name) => name.as_name_str().unwrap().to_owned(),
                        _ => "".to_owned(),
                    },
                },
                options: self.get_possibilities(self.form_ids[n]),
                readonly: is_read_only(field),
                required: is_required(field),
            },
            FieldType::CheckBox => FieldState::CheckBox {
                is_checked: match field.get(b"V") {
                    Ok(name) => name.as_name_str().unwrap() == "Yes",
                    _ => match field.get(b"AS") {
                        Ok(name) => name.as_name_str().unwrap() == "Yes",
                        _ => false,
                    },
                },
                readonly: is_read_only(field),
                required: is_required(field),
            },
            FieldType::ListBox => FieldState::ListBox {
                // V field in a list box can be either text for one option, an array for many
                // options, or null
                selected: match field.get(b"V") {
                    Ok(selection) => match *selection {
                        Object::String(ref s, StringFormat::Literal) => {
                            vec![str::from_utf8(&s).unwrap().to_owned()]
                        }
                        Object::Array(ref chosen) => {
                            let mut res = Vec::new();
                            for obj in chosen {
                                if let Object::String(ref s, StringFormat::Literal) = *obj {
                                    res.push(str::from_utf8(&s).unwrap().to_owned());
                                }
                            }
                            res
                        }
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                },
                // The options is an array of either text elements or arrays where the second
                // element is what we want
                options: match field.get(b"Opt") {
                    Ok(&Object::Array(ref options)) => options
                        .iter()
                        .map(|x| match *x {
                            Object::String(ref s, StringFormat::Literal) => {
                                str::from_utf8(&s).unwrap().to_owned()
                            }
                            Object::Array(ref arr) => {
                                if let Object::String(ref s, StringFormat::Literal) = &arr[1] {
                                    str::from_utf8(&s).unwrap().to_owned()
                                } else {
                                    String::new()
                                }
                            }
                            _ => String::new(),
                        })
                        .filter(|x| !x.is_empty())
                        .collect(),
                    _ => Vec::new(),
                },
                multiselect: {
                    let flags = ChoiceFlags::from_bits_truncate(get_field_flags(field));
                    flags.intersects(ChoiceFlags::MULTISELECT)
                },
                readonly: is_read_only(field),
                required: is_required(field),
            },
            FieldType::ComboBox => FieldState::ComboBox {
                // V field in a list box can be either text for one option, an array for many
                // options, or null
                selected: match field.get(b"V") {
                    Ok(selection) => match *selection {
                        Object::String(ref s, StringFormat::Literal) => {
                            vec![str::from_utf8(&s).unwrap().to_owned()]
                        }
                        Object::Array(ref chosen) => {
                            let mut res = Vec::new();
                            for obj in chosen {
                                if let Object::String(ref s, StringFormat::Literal) = *obj {
                                    res.push(str::from_utf8(&s).unwrap().to_owned());
                                }
                            }
                            res
                        }
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                },
                // The options is an array of either text elements or arrays where the second
                // element is what we want
                options: match field.get(b"Opt") {
                    Ok(&Object::Array(ref options)) => options
                        .iter()
                        .map(|x| match *x {
                            Object::String(ref s, StringFormat::Literal) => {
                                str::from_utf8(&s).unwrap().to_owned()
                            }
                            Object::Array(ref arr) => {
                                if let Object::String(ref s, StringFormat::Literal) = &arr[1] {
                                    str::from_utf8(&s).unwrap().to_owned()
                                } else {
                                    String::new()
                                }
                            }
                            _ => String::new(),
                        })
                        .filter(|x| !x.is_empty())
                        .collect(),
                    _ => Vec::new(),
                },
                editable: {
                    let flags = ChoiceFlags::from_bits_truncate(get_field_flags(field));

                    flags.intersects(ChoiceFlags::EDIT)
                },
                readonly: is_read_only(field),
                required: is_required(field),
            },
            FieldType::Text => FieldState::Text {
                text: match field.get(b"V") {
                    Ok(&Object::String(ref s, StringFormat::Literal)) => {
                        str::from_utf8(&s.clone()).unwrap().to_owned()
                    }
                    _ => "".to_owned(),
                },
                readonly: is_read_only(field),
                required: is_required(field),
            },
            FieldType::Unknown => FieldState::Unknown,
        }
    }

    /// If the field at index `n` is a text field, fills in that field with the text `s`.
    /// If it is not a text field, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_text(&mut self, n: usize, s: String) -> Result<(), ValueError> {
        match self.get_state(n) {
            FieldState::Text { .. } => {
                let field = self
                    .doc
                    .objects
                    .get_mut(&self.form_ids[n])
                    .unwrap()
                    .as_dict_mut()
                    .unwrap();

                field.set("V", Object::string_literal(s.into_bytes()));

                // Regenerate text appearance confoming the new text but ignore the result
                let _ = self.regenerate_text_appearance(n);

                Ok(())
            }
            _ => Err(ValueError::TypeMismatch),
        }
    }

    /// Regenerates the appearance for the field at index `n` due to an alteration of the
    /// original TextField value, the AP will be updated accordingly.
    ///
    /// # Incomplete
    /// This function is not exhaustive as not parse the original TextField orientation
    /// or the text alignment and other kind of enrichments, also doesn't discover for
    /// the global document DA.
    ///
    /// A more sophisticated parser is needed here
    fn regenerate_text_appearance(&mut self, n: usize) -> Result<(), lopdf::Error> {
        let field = {
            self.doc
                .objects
                .get(&self.form_ids[n])
                .unwrap()
                .as_dict()
                .unwrap()
        };

        // The value of the object (should be a string)
        let value = field.get(b"V")?.to_owned();

        // The default appearance of the object (should be a string)
        let da = field.get(b"DA")?.to_owned();

        // The default appearance of the object (should be a string)
        let rect = field
            .get(b"Rect")?
            .as_array()?
            .iter()
            .map(|object| {
                object
                    .as_f64()
                    .unwrap_or(object.as_i64().unwrap_or(0) as f64)
            })
            .collect::<Vec<_>>();

        // Gets the object stream
        let object_id = field.get(b"AP")?.as_dict()?.get(b"N")?.as_reference()?;
        let stream = self.doc.get_object_mut(object_id)?.as_stream_mut()?;

        // Decode and get the content, even if is compressed
        let mut content = {
            if let Ok(content) = stream.decompressed_content() {
                Content::decode(&content)?
            } else {
                Content::decode(&stream.content)?
            }
        };

        // Ignored operators
        let ignored_operators = vec![
            "bt", "tc", "tw", "tz", "g", "tr", "tf", "tj", "et", "q", "bmc", "emc",
        ];

        // Remove these ignored operators as we have to generate the text and fonts again
        content.operations.retain(|operation| {
            !ignored_operators.contains(&operation.operator.to_lowercase().as_str())
        });

        // Let's construct the text widget
        content.operations.append(&mut vec![
            Operation::new("BMC", vec!["Tx".into()]),
            Operation::new("q", vec![]),
            Operation::new("BT", vec![]),
        ]);

        // The default font object (/Helv 12 Tf 0 g)
        let default_font = ("Helv", 12, 0, "g");

        // Build the font basing on the default appearance, if exists, if not,
        // assume a default font (surely to be improved!)
        let font = match da {
            Object::String(ref bytes, _) => {
                let values = from_utf8(bytes)?
                    .trim_start_matches('/')
                    .split(' ')
                    .collect::<Vec<_>>();

                if values.len() != 5 {
                    default_font
                } else {
                    (
                        values[0],
                        values[1].parse::<i32>().unwrap_or(0),
                        values[3].parse::<i32>().unwrap_or(0),
                        values[4],
                    )
                }
            }
            _ => default_font,
        };

        // Define some helping font variables
        let font_name = font.0;
        let font_size = font.1;
        let font_color = (font.2, font.3);

        // Set the font type and size and color
        content.operations.append(&mut vec![
            Operation::new("Tf", vec![font_name.into(), font_size.into()]),
            Operation::new(font_color.1, vec![font_color.0.into()]),
        ]);

        // Calcolate the text offset
        let x = 3.0; // Suppose this fixed offset as we should have known the border here
        let y = 0.5 * (rect[3] - rect[1]) - 0.4 * font_size as f64; // Formula picked up from Poppler

        // Set the text bounds, first are fixed at "1 0 0 1" and then the calculated x,y
        content.operations.append(&mut vec![Operation::new(
            "Tm",
            vec![1.into(), 0.into(), 0.into(), 1.into(), x.into(), y.into()],
        )]);

        // Set the text value and some finalizing operations
        content.operations.append(&mut vec![
            Operation::new("Tj", vec![value]),
            Operation::new("ET", vec![]),
            Operation::new("Q", vec![]),
            Operation::new("EMC", vec![]),
        ]);

        // Set the new content to the original stream and compress it
        if let Ok(encoded_content) = content.encode() {
            stream.set_plain_content(encoded_content);
            let _ = stream.compress();
        }

        Ok(())
    }

    /// If the field at index `n` is a checkbox field, toggles the check box based on the value
    /// `is_checked`.
    /// If it is not a checkbox field, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_check_box(&mut self, n: usize, is_checked: bool) -> Result<(), ValueError> {
        match self.get_state(n) {
            FieldState::CheckBox { .. } => {
                let field = self
                    .doc
                    .objects
                    .get_mut(&self.form_ids[n])
                    .unwrap()
                    .as_dict_mut()
                    .unwrap();

                let on = get_on_value(field);
                let state = Object::Name(
                    if is_checked { on.as_str() } else { "Off" }
                        .to_owned()
                        .into_bytes(),
                );

                field.set("V", state.clone());
                field.set("AS", state);

                Ok(())
            }
            _ => Err(ValueError::TypeMismatch),
        }
    }

    /// If the field at index `n` is a radio field, toggles the radio button based on the value
    /// `choice`
    /// If it is not a radio button field or the choice is not a valid option, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_radio(&mut self, n: usize, choice: String) -> Result<(), ValueError> {
        match self.get_state(n) {
            FieldState::Radio { options, .. } => {
                if options.contains(&choice) {
                    let field = self
                        .doc
                        .objects
                        .get_mut(&self.form_ids[n])
                        .unwrap()
                        .as_dict_mut()
                        .unwrap();
                    field.set("V", Object::Name(choice.into_bytes()));
                    Ok(())
                } else {
                    Err(ValueError::InvalidSelection)
                }
            }
            _ => Err(ValueError::TypeMismatch),
        }
    }

    /// If the field at index `n` is a listbox field, selects the options in `choice`
    /// If it is not a listbox field or one of the choices is not a valid option, or if too many choices are selected, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_list_box(&mut self, n: usize, choices: Vec<String>) -> Result<(), ValueError> {
        match self.get_state(n) {
            FieldState::ListBox {
                options,
                multiselect,
                ..
            } => {
                if choices.iter().fold(true, |a, h| options.contains(h) && a) {
                    if !multiselect && choices.len() > 1 {
                        Err(ValueError::TooManySelected)
                    } else {
                        let field = self
                            .doc
                            .objects
                            .get_mut(&self.form_ids[n])
                            .unwrap()
                            .as_dict_mut()
                            .unwrap();
                        match choices.len() {
                            0 => field.set("V", Object::Null),
                            1 => field.set(
                                "V",
                                Object::String(
                                    choices[0].clone().into_bytes(),
                                    StringFormat::Literal,
                                ),
                            ),
                            _ => field.set(
                                "V",
                                Object::Array(
                                    choices
                                        .iter()
                                        .map(|x| {
                                            Object::String(
                                                x.clone().into_bytes(),
                                                StringFormat::Literal,
                                            )
                                        })
                                        .collect(),
                                ),
                            ),
                        };
                        Ok(())
                    }
                } else {
                    Err(ValueError::InvalidSelection)
                }
            }
            _ => Err(ValueError::TypeMismatch),
        }
    }

    /// If the field at index `n` is a combobox field, selects the options in `choice`
    /// If it is not a combobox field or one of the choices is not a valid option, or if too many choices are selected, returns ValueError
    ///
    /// # Panics
    /// Will panic if n is larger than the number of fields
    pub fn set_combo_box(&mut self, n: usize, choice: String) -> Result<(), ValueError> {
        match self.get_state(n) {
            FieldState::ComboBox {
                options, editable, ..
            } => {
                if options.contains(&choice) || editable {
                    let field = self
                        .doc
                        .objects
                        .get_mut(&self.form_ids[n])
                        .unwrap()
                        .as_dict_mut()
                        .unwrap();
                    field.set(
                        "V",
                        Object::String(choice.into_bytes(), StringFormat::Literal),
                    );
                    Ok(())
                } else {
                    Err(ValueError::InvalidSelection)
                }
            }
            _ => Err(ValueError::TypeMismatch),
        }
    }

    /// Saves the form to the specified path
    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<(), io::Error> {
        self.doc.save(path).map(|_| ())
    }

    /// Saves the form to the specified path
    pub fn save_to<W: Write>(&mut self, target: &mut W) -> Result<(), io::Error> {
        self.doc.save_to(target)
    }

    fn get_possibilities(&self, oid: ObjectId) -> Vec<String> {
        let mut res = Vec::new();
        let kids_obj = self
            .doc
            .objects
            .get(&oid)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Kids");
        if let Ok(&Object::Array(ref kids)) = kids_obj {
            for (i, kid) in kids.iter().enumerate() {
                let mut found = false;
                if let Ok(&Object::Dictionary(ref appearance_states)) =
                    kid.deref(&self.doc).unwrap().as_dict().unwrap().get(b"AP")
                {
                    if let Ok(&Object::Dictionary(ref normal_appearance)) =
                        appearance_states.get(b"N")
                    {
                        for (key, _) in normal_appearance {
                            if key != b"Off" {
                                res.push(from_utf8(key).unwrap_or("").to_owned());
                                found = true;
                                break;
                            }
                        }
                    }
                }

                if !found {
                    res.push(i.to_string());
                }
            }
        }

        res
    }
}
