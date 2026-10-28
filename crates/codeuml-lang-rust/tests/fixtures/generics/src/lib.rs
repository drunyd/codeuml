pub struct Container<'a, T: Clone + Send = String>
where
    T: Default,
{
    pub value: T,
    marker: &'a str,
}

pub(crate) struct CrateVisible;

pub(super) struct SuperVisible;

pub(in crate::nested) struct Restricted;

impl<T> Container<'_, T>
where
    T: Clone,
{
    pub fn value(&self) -> &T {
        &self.value
    }
}
