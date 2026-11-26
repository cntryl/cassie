use crate::benchmark::{PgwireFrontendCodecKernel, PgwireRowCodecKernel};

#[test]
fn should_encode_a_pgwire_data_row_frame() {
    // Arrange
    let row = PgwireRowCodecKernel::sample(7);

    // Act
    let encoded = row.encode();

    // Assert
    assert_eq!(encoded.first(), Some(&b'D'));
    assert!(encoded.len() >= 7);
}

#[test]
fn should_decode_each_supported_frontend_frame_fixture() {
    // Arrange
    let fixture = PgwireFrontendCodecKernel::with_frames(10);

    // Act
    let decoded_payload_bytes = fixture.decode();

    // Assert
    assert_eq!(fixture.frame_count(), 10);
    assert!(decoded_payload_bytes > 0);
}
