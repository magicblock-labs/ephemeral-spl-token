use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = any)]
pub struct DepositArgs {
    pub amount: u64,
}

#[cfg(test)]
mod tests {
    use wheels::layout::{Decodable, Encodable};

    use super::DepositArgs;

    #[test]
    fn deposit_bytes_decode_at_every_offset() {
        let amount = 0x0807_0605_0403_0201;
        let expected = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(DepositArgs { amount }.encode().unwrap(), expected);

        let mut buffer = [0; DepositArgs::DATA_LEN + 7];
        for start in 0..8 {
            let payload = &mut buffer[start..start + expected.len()];
            payload.copy_from_slice(&expected);
            assert_eq!(DepositArgs::decode(payload).unwrap().amount(), amount);
        }
    }
}
