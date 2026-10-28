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
}
