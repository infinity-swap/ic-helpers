use std::fmt;

use ic_exports::ic_cdk::export::candid::types::internal::TypeContainer;
use ic_exports::ic_cdk::export::candid::types::Type;
use ic_exports::ic_cdk::export::candid::{self};

pub struct Idl {
    pub env: TypeContainer,
    pub actor: Type,
}

impl fmt::Display for Idl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Idl: {}", self.actor)
    }
}

impl Idl {
    pub fn new(env: TypeContainer, actor: Type) -> Self {
        Self { env, actor }
    }

    pub fn merge(&mut self, other: &Self) {
        self.env = candid::types::internal::TypeContainer {
            env: self.env.env.merge(&other.env.env).unwrap().clone(),
        };

        match (&mut self.actor, &other.actor) {
            (Type::Class(ref class, left), Type::Service(ref right)) => match **left {
                Type::Service(ref mut left) => {
                    left.extend(right.clone());
                    self.actor = Type::Class(class.to_vec(), Box::new(Type::Service(left.clone())));
                }
                _ => {
                    panic!("type {left:#?} is not a service")
                }
            },
            (Type::Service(ref mut left), Type::Class(ref class, right)) => match **right {
                Type::Service(ref right) => {
                    left.extend(right.clone());
                    self.actor = Type::Class(class.to_vec(), Box::new(Type::Service(left.clone())));
                }
                _ => {
                    panic!("type {right:#?} is not a service")
                }
            },
            (Type::Service(left), Type::Service(right)) => {
                left.extend(right.clone());
            }
            (l @ Type::Class(_, _), r @ Type::Class(_, _)) => {
                panic!("cannot merge two candid classes: self:\n{l:#?}\nother:\n{r:#?}")
            }
            (l, r) => {
                panic!(
                    "wrong candid types were generated by the macro: self:\n{l:#?}\nother:\n{r:#?}"
                )
            }
        }
    }
}
