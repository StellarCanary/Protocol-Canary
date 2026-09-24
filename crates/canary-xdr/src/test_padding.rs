#[cfg(test)]
mod tests {
    use stellar_xdr::{StellarValue, ReadXdr, Limits};

    #[test]
    fn test_padding() {
        let hex = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000000014101020300000000";
        let bytes = hex::decode(hex).unwrap();
        let b64 = base64::encode(&bytes);
        println!("Base64: {}", b64);
        
        match StellarValue::from_xdr_base64(&b64, Limits::none()) {
            Ok(v) => println!("Success: {:?}", v),
            Err(e) => println!("Error: {:?}", e),
        }
        panic!("Show output"); // to see stdout in cargo test
    }
}
