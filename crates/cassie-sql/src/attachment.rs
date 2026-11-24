//! Typed structural conversion between syntax-only and engine AST attachments.

/// Rebinds every nested AST node while preserving syntax fields and metadata.
pub trait MapAttachment {
    type Rebound<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B>;
}

impl<T: MapAttachment> MapAttachment for Box<T> {
    type Rebound<B> = Box<T::Rebound<B>>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> { Box::new((*self).rebind_attachment::<B>()) }
}

impl<T: MapAttachment> MapAttachment for Option<T> {
    type Rebound<B> = Option<T::Rebound<B>>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> { self.map(|value| value.rebind_attachment::<B>()) }
}

impl<T: MapAttachment> MapAttachment for Vec<T> {
    type Rebound<B> = Vec<T::Rebound<B>>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        self.into_iter().map(|value| value.rebind_attachment::<B>()).collect()
    }
}

pub fn rebind_attachment<B, T: MapAttachment>(value: T) -> T::Rebound<B> {
    value.rebind_attachment::<B>()
}

mod nodes_a;
mod nodes_b;
