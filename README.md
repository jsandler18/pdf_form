# Fill PDF Forms
A library to programatically identify and fill out PDF forms.

**WARNING**: This is a fork of the original "pdf_form" crate made by [jsandler18](https://github.com/jsandler18), including all improvements made by [malte-v](https://github.com/malte-v).

## Example Code
Read a PDF and discover the form fields
```rust
extern crate pdf_form;
use pdf_form::{Form, FieldType};

// Load the pdf into a form from a path
let form = Form::load("path/to/pdf").unwrap();
// Get all types of the form fields (e.g. Text, Radio, etc) in a Vector
let field_types = form.get_all_types();
// Print the types
for type in field_types {
    println!("{:?}", type);
};

```

Write to the form fields
```rust
extern crate pdf_form;
use pdf_form::{Form, FieldState};

// Load the pdf into a form from a path
let mut form = Form::load("path/to/pdf").unwrap();
form.set_text(0, String::from("filling the field"));
form.save("path/to/new/pdf");

```

