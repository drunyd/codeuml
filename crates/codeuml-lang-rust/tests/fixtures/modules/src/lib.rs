pub mod external;
pub mod second;
pub mod other;
pub mod directory;

pub mod inline_outer {
    pub mod inline_inner {
        pub struct Deep;
    }
}

pub struct RootType;
