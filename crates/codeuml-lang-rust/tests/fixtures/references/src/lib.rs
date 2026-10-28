use crate::models::{Customer, Order as Purchase};
use crate::other::*;

pub mod models {
    pub struct Customer;
    pub struct Order;
}

pub mod other {
    pub struct Extra;
}

pub trait Marker {
    type Item;
}

pub trait GenericAssoc {
    type Item<T>;
}

pub trait Repo<T: Marker> {
    fn find(&self, input: Customer) -> Option<Customer>;
}

pub struct Fields<T: Marker> {
    pub customer: Customer,
    pub purchase: Purchase,
    pub explicit: crate::models::Customer,
    pub generic: Vec<Customer>,
    pub nested: Option<Box<Customer>>,
    pub reference: &'static mut Customer,
    pub tuple: (Customer, u32),
    pub array: [Customer; 2],
    pub slice: &'static [Customer],
    pub pointer: *const Customer,
    pub value: T,
    pub qualified: <T as Marker>::Item,
    pub callback: fn(Customer) -> Customer,
}

pub struct WhereBound<U>
where
    U: Marker,
{
    pub value: U,
}

pub type Alias = Customer;
pub type DeepAlias = Vec<Alias>;

impl<T: Marker> Fields<T> {
    pub fn convert(&self, input: &Purchase) -> Customer {
        Customer
    }
    pub fn constrained(&self) -> impl Iterator<Item: Marker> {
        loop {}
    }
    pub fn associated(&self) -> impl GenericAssoc<Item<Customer> = Customer> {
        loop {}
    }
}

pub struct BoundShapes {
    pub trait_object: &'static dyn Marker,
    pub global: ::references::models::Customer,
    pub parenthesized: (Customer),
    pub lifetime_and_const: ArrayHolder<'static, 3>,
}

pub struct ArrayHolder<'a, const N: usize> {
    pub value: &'a [Customer; N],
}

pub type Never = !;
pub type Infer = _;

pub mod nested_imports {
    use super::models as ModelRoot;
    use super::models::{self, Customer as LocalCustomer, Order};
    use super::other::*;

    pub struct Imported {
        pub qualified: models::Customer,
        pub renamed: LocalCustomer,
        pub grouped: Order,
        pub globbed: Extra,
        pub self_renamed: ModelRoot::Customer,
        pub self_import: LocalHere,
    }

    pub struct Here;
    use self::Here as LocalHere;

    pub mod deeper {
        use super::super::models::Customer as RootCustomer;
        pub struct DeepImport {
            pub value: RootCustomer,
        }
    }
}

pub mod duplicate_one {
    pub struct Duplicated;
}
pub mod duplicate_two {
    pub struct Duplicated;
}
pub mod ambiguous_impl {
    impl Duplicated {
        pub fn skipped(&self) {}
    }
}

impl<T: Marker> Fields<T> {
    pub fn iterator(&self) -> impl Iterator<Item = Customer> {
        loop {}
    }
    pub fn convert(&self, input: &Purchase) -> Customer {
        Customer
    }
}
