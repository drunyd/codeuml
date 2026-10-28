pub trait Entity {
    fn id(&self) -> u64;
    fn create(name: String) -> Self;
    fn enabled(&self) -> bool {
        true
    }
}

pub struct User {
    pub id: u64,
}

impl User {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn id(&self) -> u64 {
        self.id
    }
}

impl Entity for User {
    fn id(&self) -> u64 {
        self.id
    }

    fn create(name: String) -> Self {
        let _ = name;
        Self { id: 0 }
    }
}

pub struct Widget;

impl ExternalTrait for Widget {}
