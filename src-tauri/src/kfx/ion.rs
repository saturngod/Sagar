//! Minimal Amazon Ion binary writer used by the KFX container format.

#[derive(Debug, Clone, PartialEq)]
pub enum Ion {
    Null,
    Bool(bool),
    Int(i64),
    /// Written as an Ion decimal rounded to six fractional digits.
    Decimal(f64),
    Symbol(u64),
    Str(String),
    List(Vec<Ion>),
    Struct(Vec<(u64, Ion)>),
    Annotated(Vec<u64>, Box<Ion>),
}

impl Ion {
    pub fn str(value: impl Into<String>) -> Self {
        Ion::Str(value.into())
    }
}

#[derive(Default)]
pub struct IonWriter {
    pub data: Vec<u8>,
}

const BVM: [u8; 4] = [0xe0, 0x01, 0x00, 0xea];

impl IonWriter {
    pub fn write_bvm(&mut self) {
        self.data.extend_from_slice(&BVM);
    }

    pub fn write(&mut self, value: &Ion) {
        match value {
            Ion::Null => self.data.push(0x0f),
            Ion::Bool(value) => self.data.push(if *value { 0x11 } else { 0x10 }),
            Ion::Int(value) => self.write_int(*value),
            Ion::Decimal(value) => self.write_decimal(*value),
            Ion::Symbol(id) => {
                if *id == 0 {
                    self.data.push(0x70);
                } else {
                    self.write_bytes(7, &minimal_bytes(*id));
                }
            }
            Ion::Str(string) => self.write_bytes(8, string.as_bytes()),
            Ion::List(values) => {
                let mut inner = IonWriter::default();
                for value in values {
                    inner.write(value);
                }
                self.write_bytes(11, &inner.data);
            }
            Ion::Struct(fields) => {
                let mut inner = IonWriter::default();
                for (field, value) in fields {
                    inner.write_var_uint(*field);
                    inner.write(value);
                }
                self.write_bytes(13, &inner.data);
            }
            Ion::Annotated(annotations, value) => {
                let mut annotation_bytes = IonWriter::default();
                for annotation in annotations {
                    annotation_bytes.write_var_uint(*annotation);
                }
                let mut inner = IonWriter::default();
                inner.write_var_uint(annotation_bytes.data.len() as u64);
                inner.data.extend_from_slice(&annotation_bytes.data);
                inner.write(value);
                self.write_bytes(14, &inner.data);
            }
        }
    }

    fn write_int(&mut self, value: i64) {
        if value == 0 {
            self.data.push(0x20);
            return;
        }
        let kind = if value < 0 { 3 } else { 2 };
        self.write_bytes(kind, &minimal_bytes(value.unsigned_abs()));
    }

    fn write_decimal(&mut self, value: f64) {
        if !value.is_finite() || value == 0.0 {
            self.data.extend_from_slice(&[0x51, 0x80]);
            return;
        }
        let negative = value < 0.0;
        let mut coefficient = (value.abs() * 1e6).round() as u64;
        let mut exponent: i32 = -6;
        if coefficient == 0 {
            exponent = 0;
        }
        while coefficient != 0 && coefficient.is_multiple_of(10) && exponent < 0 {
            coefficient /= 10;
            exponent += 1;
        }
        let mut payload = vec![
            0x80 | if exponent < 0 { 0x40 } else { 0 } | (exponent.unsigned_abs() as u8 & 0x3f),
        ];
        if coefficient > 0 {
            let mut bytes = minimal_bytes(coefficient);
            if negative {
                if bytes[0] & 0x80 != 0 {
                    bytes.insert(0, 0x80);
                } else {
                    bytes[0] |= 0x80;
                }
            } else if bytes[0] & 0x80 != 0 {
                bytes.insert(0, 0);
            }
            payload.extend_from_slice(&bytes);
        }
        self.write_bytes(5, &payload);
    }

    fn write_bytes(&mut self, kind: u8, bytes: &[u8]) {
        if bytes.len() < 14 {
            self.data.push((kind << 4) | bytes.len() as u8);
        } else {
            self.data.push((kind << 4) | 14);
            self.write_var_uint(bytes.len() as u64);
        }
        self.data.extend_from_slice(bytes);
    }

    pub fn write_var_uint(&mut self, value: u64) {
        if value == 0 {
            self.data.push(0x80);
            return;
        }
        let mut value = value;
        let mut groups = Vec::new();
        while value > 0 {
            groups.push((value & 0x7f) as u8);
            value >>= 7;
        }
        for index in (0..groups.len()).rev() {
            self.data
                .push(groups[index] | if index == 0 { 0x80 } else { 0 });
        }
    }
}

fn minimal_bytes(value: u64) -> Vec<u8> {
    value
        .to_be_bytes()
        .into_iter()
        .skip_while(|byte| *byte == 0)
        .collect()
}

/// A complete Ion binary value stream: version marker plus one value.
pub fn ion_data(value: &Ion, annotation: Option<u64>) -> Vec<u8> {
    let mut writer = IonWriter::default();
    writer.write_bvm();
    match annotation {
        Some(annotation) => {
            writer.write(&Ion::Annotated(vec![annotation], Box::new(value.clone())))
        }
        None => writer.write(value),
    }
    writer.data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(value: Ion) -> Vec<u8> {
        let mut writer = IonWriter::default();
        writer.write(&value);
        writer.data
    }

    #[test]
    fn encodes_scalars() {
        assert_eq!(bytes(Ion::Int(0)), vec![0x20]);
        assert_eq!(bytes(Ion::Int(300)), vec![0x22, 0x01, 0x2c]);
        assert_eq!(bytes(Ion::Int(-1)), vec![0x31, 0x01]);
        assert_eq!(bytes(Ion::Symbol(4)), vec![0x71, 0x04]);
        assert_eq!(bytes(Ion::str("ab")), vec![0x82, b'a', b'b']);
        assert_eq!(bytes(Ion::Decimal(0.0)), vec![0x51, 0x80]);
        // 1.2 = 12 x 10^-1
        assert_eq!(bytes(Ion::Decimal(1.2)), vec![0x52, 0xc1, 0x0c]);
        assert_eq!(bytes(Ion::Decimal(-1.5)), vec![0x52, 0xc1, 0x8f]);
        assert_eq!(bytes(Ion::Decimal(2.0)), vec![0x52, 0x80, 0x02]);
    }

    #[test]
    fn encodes_long_and_nested_values() {
        let long = "x".repeat(20);
        let encoded = bytes(Ion::str(long));
        assert_eq!(&encoded[..2], &[0x8e, 0x94]);
        assert_eq!(
            bytes(Ion::Struct(vec![(4, Ion::Bool(true))])),
            vec![0xd2, 0x84, 0x11]
        );
        assert_eq!(
            bytes(Ion::Annotated(vec![3], Box::new(Ion::Null))),
            vec![0xe3, 0x81, 0x83, 0x0f]
        );
    }
}
