//! Re-encoding a previously decoded XDR value.

use stellar_xdr::{Limits, WriteXdr};

use crate::decoder::DecodedValue;

/// Re-encodes `value` back to base64 XDR.
pub fn encode(value: &DecodedValue) -> Result<String, stellar_xdr::Error> {
    match value {
        DecodedValue::StellarValue(v) => v.to_xdr_base64(Limits::none()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{decode, DecodedValue, XdrTypeName};
    use stellar_xdr::StellarValue;

    #[test]
    fn encoding_a_decoded_value_reproduces_the_same_value() {
        let value = StellarValue::default();
        let base64 = encode(&DecodedValue::StellarValue(value.clone())).expect("encodes");

        let decoded = decode(XdrTypeName::StellarValue, &base64).expect("decodes");
        let re_encoded = encode(&decoded).expect("re-encodes");

        assert_eq!(base64, re_encoded);
        match decoded {
            DecodedValue::StellarValue(v) => assert_eq!(v, value),
        }
    }
}
