use super::{key, prefix, FAMILY_OBJECT_COMMENT};

pub(in crate::midge::adapter) fn object_comment_key(class_oid: u32, object_oid: u32, object_sub_id: i32) -> Vec<u8> {
    let class_oid = class_oid.to_be_bytes();
    let object_oid = object_oid.to_be_bytes();
    let object_sub_id = object_sub_id.to_be_bytes();
    key(
        FAMILY_OBJECT_COMMENT,
        &[
            class_oid.as_slice(),
            object_oid.as_slice(),
            object_sub_id.as_slice(),
        ],
    )
}

pub(in crate::midge::adapter) fn object_comment_object_prefix(class_oid: u32, object_oid: u32) -> Vec<u8> {
    let class_oid = class_oid.to_be_bytes();
    let object_oid = object_oid.to_be_bytes();
    prefix(
        FAMILY_OBJECT_COMMENT,
        &[class_oid.as_slice(), object_oid.as_slice()],
    )
}

pub(in crate::midge::adapter) fn object_comment_prefix() -> Vec<u8> {
    prefix(FAMILY_OBJECT_COMMENT, &[])
}
