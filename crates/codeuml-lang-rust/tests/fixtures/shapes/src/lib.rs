pub struct Named {
    pub id: u64,
    name: String,
}

pub struct Tuple(pub u32, String);

pub struct Unit;

pub enum Status {
    Active,
    Suspended { reason: String },
    Pair(u8, u16),
    Empty,
}

pub union Bits {
    pub int: u32,
    pub float: f32,
}

pub type Alias<T> = Vec<T>;
