#[derive(Debug)]
pub struct ButtonData {
    pub assign_type: u8,
    pub behavior: u8,
    pub cc_note_number: u8,
    pub off_value: u8,
    pub on_value: u8,
}
impl ButtonData {
    fn from_bytes(data: &[u8]) -> ButtonData {
        if data.len() != 6 {
            panic!("Invalid data: {:0X?}", data)
        }
        ButtonData {
            assign_type: data[0],
            behavior: data[1],
            cc_note_number: data[2],
            off_value: data[3],
            on_value: data[4],
            // reserved: data[5]
        }
    }
}
#[derive(Debug)]
pub struct AnalogData {
    pub assign_type: u8,
    pub cc_note_number: u8,
    pub min_value: u8,
    pub max_value: u8,
}
impl AnalogData {
    fn from_bytes(data: &[u8]) -> AnalogData {
        if data.len() != 6 {
            panic!("Invalid data: {:0X?}", data)
        }
        AnalogData {
            assign_type: data[0],
            // reserved: data[1]
            cc_note_number: data[2],
            min_value: data[3],
            max_value: data[4],
            // reserved: data[5]
        }
    }
}
#[derive(Debug)]
pub struct GroupData {
    pub group_midi_ch: u8,
    pub slider: AnalogData,
    pub knob: AnalogData,
    pub solo_button: ButtonData,
    pub mute_button: ButtonData,
    pub rec_button: ButtonData,
}
impl GroupData {
    fn from_bytes(data: &[u8]) -> GroupData {
        GroupData {
            group_midi_ch: data[0],
            slider: AnalogData::from_bytes(&data[1..=6]),
            knob: AnalogData::from_bytes(&data[7..=12]),
            solo_button: ButtonData::from_bytes(&data[13..=18]),
            mute_button: ButtonData::from_bytes(&data[19..=24]),
            rec_button: ButtonData::from_bytes(&data[25..=30]),
        }
    }
}
#[derive(Debug)]
pub struct SceneData {
    pub global_midi_ch: u8,
    pub control_mode: u8,
    pub led_mode: u8,
    pub group: [GroupData; 8],
    pub transport_button_midi_ch: u8,
    pub prev_track_button: ButtonData,
    pub next_track_button: ButtonData,
    pub cycle_button: ButtonData,
    pub marker_set_button: ButtonData,
    pub prev_marker_button: ButtonData,
    pub next_marker_button: ButtonData,
    pub rew_button: ButtonData,
    pub ff_button: ButtonData,
    pub stop_button: ButtonData,
    pub play_button: ButtonData,
    pub rec_button: ButtonData,
    pub custom_daw_assign: [u8; 5],
}
fn decode_block(data: &[u8], buf: &mut Vec<u8>) {
    for i in 0..7 {
        if i + 1 < data.len() {
            buf.push(data[1 + i] | (data[0] & 1 << i) << (7 - i));
        }
    }
}
fn decode(data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity((data.len() + 7) / 8 * 7);
    let mut i = 0;
    while i < data.len() / 8 {
        decode_block(&data[(i * 8)..(i * 8 + 8)], &mut buf);
        i += 1;
    }
    decode_block(&data[i * 8..data.len()], &mut buf);
    buf
}
impl SceneData {
    pub fn from_encoded_bytes(data: &[u8]) -> Option<SceneData> {
        let orig_len = data.len();
        let data = decode(data);
        println!(
            "Decoded: orig_len={}, len={}, {:02X?}",
            orig_len,
            data.len(),
            data
        );
        Self::from_bytes(&data)
    }
    fn from_bytes(data: &[u8]) -> Option<SceneData> {
        if data.len() != 339 {
            return None;
        }
        Some(SceneData {
            global_midi_ch: data[0],
            control_mode: data[1],
            led_mode: data[2],
            group: [
                GroupData::from_bytes(&data[3..=33]),
                GroupData::from_bytes(&data[34..=64]),
                GroupData::from_bytes(&data[65..=95]),
                GroupData::from_bytes(&data[96..=126]),
                GroupData::from_bytes(&data[127..=157]),
                GroupData::from_bytes(&data[158..=188]),
                GroupData::from_bytes(&data[189..=219]),
                GroupData::from_bytes(&data[220..=250]),
            ],
            transport_button_midi_ch: data[251],
            prev_track_button: ButtonData::from_bytes(&data[252..=257]),
            next_track_button: ButtonData::from_bytes(&data[258..=263]),
            cycle_button: ButtonData::from_bytes(&data[264..=269]),
            marker_set_button: ButtonData::from_bytes(&data[270..=275]),
            prev_marker_button: ButtonData::from_bytes(&data[276..=281]),
            next_marker_button: ButtonData::from_bytes(&data[282..=287]),
            rew_button: ButtonData::from_bytes(&data[288..=293]),
            ff_button: ButtonData::from_bytes(&data[294..=299]),
            stop_button: ButtonData::from_bytes(&data[300..=305]),
            play_button: ButtonData::from_bytes(&data[306..=311]),
            rec_button: ButtonData::from_bytes(&data[312..=317]),
            custom_daw_assign: [data[318], data[319], data[320], data[321], data[322]],
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_decode() {
        assert_eq!(decode(&[]), &[]);
        assert_eq!(decode(&[0x00]), &[]);
        assert_eq!(decode(&[0x00, 0x01]), &[0x01]);
        assert_eq!(decode(&[0x01, 0x01, 0x02]), &[0x81, 0x02]);
        assert_eq!(decode(&[0x02, 0x01, 0x02]), &[0x01, 0x82]);
        assert_eq!(
            decode(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]),
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]
        );
        assert_eq!(
            decode(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x01]),
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]
        );
        assert_eq!(
            decode(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x01, 0x01]),
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x81]
        );
    }
}
