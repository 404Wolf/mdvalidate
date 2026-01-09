#[allow(dead_code)]
use tracing::instrument;

use crate::mdschema::validator::{
    node_walker::ValidationResult, validator_walker::ValidatorWalker,
};

pub(super) mod code;
pub(super) mod headings;
pub(super) mod links;
pub(super) mod lists;
pub(super) mod matchers;
pub(crate) mod nodes;
pub(super) mod tables;
pub(super) mod textual;
pub(super) mod textual_container;

pub trait ValidatorImpl {
    fn validate_impl(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult;
}

pub trait Validator {
    fn validate(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult;
}

impl<T: ValidatorImpl> Validator for T {
    #[instrument(skip_all, level = "trace", fields(
        v = std::any::type_name::<T>().strip_prefix("mdvalidate::mdschema::validator::node_walker::validators::").unwrap_or(std::any::type_name::<T>()),
        i = %walker.input_cursor().descendant_index(),
        s = %walker.schema_cursor().descendant_index(),
    ), ret)]
    fn validate(walker: &ValidatorWalker, got_eof: bool) -> ValidationResult {
        Self::validate_impl(walker, got_eof)
    }
}

#[cfg(test)]
mod test_utils {
    use tree_sitter::{Node, Tree, TreeCursor};

    use crate::mdschema::validator::{
        node_walker::utils::pretty_print_cursor_pair, ts_utils::parse_markdown,
        validator_walker::ValidatorWalker,
    };

    use super::*;

    pub struct ValidatorTester<'a, V: Validator> {
        _phantom: std::marker::PhantomData<V>,
        schema_tree: Tree,
        schema_str: &'a str,
        input_tree: Tree,
        input_str: &'a str,
    }

    impl<'a, V: Validator> ValidatorTester<'a, V> {
        pub fn from_strs(schema_str: &'a str, input_str: &'a str) -> Self {
            let schema_tree = parse_markdown(schema_str).unwrap();
            let input_tree = parse_markdown(input_str).unwrap();

            Self {
                _phantom: std::marker::PhantomData,
                schema_tree,
                schema_str,
                input_tree,
                input_str,
            }
        }

        pub fn walk(&'_ self) -> ValidationTesterWalker<'_, V> {
            let schema_cursor = self.schema_tree.walk();
            let input_cursor = self.input_tree.walk();

            ValidationTesterWalker {
                _phantom: std::marker::PhantomData,
                schema_cursor,
                schema_str: self.schema_str,
                input_cursor,
                input_str: self.input_str,
            }
        }
    }

    pub struct ValidationTesterWalker<'a, V: Validator> {
        _phantom: std::marker::PhantomData<V>,
        schema_cursor: TreeCursor<'a>,
        schema_str: &'a str,
        input_cursor: TreeCursor<'a>,
        input_str: &'a str,
    }

    impl<'a, V: Validator> ValidationTesterWalker<'a, V> {
        pub fn validate(&mut self, got_eof: bool) -> ValidationResult {
            self.print();

            let walker = ValidatorWalker::from_cursors(
                &self.schema_cursor,
                self.schema_str,
                &self.input_cursor,
                self.input_str,
            );
            V::validate(&walker, got_eof)
        }

        pub fn validate_complete(&mut self) -> ValidationResult {
            self.validate(true)
        }

        pub fn validate_incomplete(&mut self) -> ValidationResult {
            self.validate(false)
        }

        pub fn with_input_cursor<F>(&mut self, f: F) -> &mut Self
        where
            F: FnOnce(&mut TreeCursor<'a>),
        {
            f(&mut self.input_cursor);
            self
        }

        pub fn with_schema_cursor<F>(&mut self, f: F) -> &mut Self
        where
            F: FnOnce(&mut TreeCursor<'a>),
        {
            f(&mut self.schema_cursor);
            self
        }

        /// Peek at the nodes that our cursors are currently positioned at.
        ///
        /// Calls your callback with the (schema_node, input_node).
        pub fn peek_nodes<F>(&mut self, f: F) -> &mut Self
        where
            F: Fn((&Node, &Node)),
        {
            f((&self.schema_cursor.node(), &self.input_cursor.node()));
            self
        }

        #[allow(dead_code)]
        pub fn panic_print(&mut self) -> &mut Self {
            self.print();
            panic!();
            #[allow(unreachable_code)]
            return &mut self;
        }

        pub fn print(&mut self) -> &mut Self {
            println!(
                "{}",
                pretty_print_cursor_pair(&self.schema_cursor, &self.input_cursor)
            );
            self
        }
    }

    macro_rules! delegate_tree_cursor_methods {
        ($($goto:ident($($arg:ident: $arg_ty:ty),*)),* $(,)?) => {
            #[allow(dead_code)]
            impl<'a, V: Validator> ValidationTesterWalker<'a, V> {
                $(
                    paste::paste! {
                        pub fn [<$goto _then>](&mut self, $($arg: $arg_ty),*) -> Result<&mut ValidationTesterWalker<'a, V>, ()> {
                            (self.schema_cursor.$goto($($arg),*) && self.input_cursor.$goto($($arg),*))
                                .then(|| self)
                                .ok_or(())
                        }

                        pub fn [<$goto _then_unwrap>](&mut self, $($arg: $arg_ty),*) -> &mut ValidationTesterWalker<'a, V> {
                            self.[<$goto _then>]($($arg),*).unwrap()
                        }

                        pub fn [<$goto _for_input>](&mut self, $($arg: $arg_ty),*) -> Result<&mut ValidationTesterWalker<'a, V>, ()> {
                            self.input_cursor.$goto($($arg),*)
                                .then(|| self)
                                .ok_or(())
                        }

                        pub fn [<$goto _for_input_unwrap>](&mut self, $($arg: $arg_ty),*) -> &mut ValidationTesterWalker<'a, V> {
                            self.[<$goto _for_input>]($($arg),*).unwrap()
                        }

                        pub fn [<$goto _for_schema>](&mut self, $($arg: $arg_ty),*) -> Result<&mut ValidationTesterWalker<'a, V>, ()> {
                            self.schema_cursor.$goto($($arg),*)
                                .then(|| self)
                                .ok_or(())
                        }

                        pub fn [<$goto _for_schema_unwrap>](&mut self, $($arg: $arg_ty),*) -> &mut ValidationTesterWalker<'a, V> {
                            self.[<$goto _for_schema>]($($arg),*).unwrap()
                        }
                    }
                )*
            }
        };
    }

    delegate_tree_cursor_methods! {
        goto_first_child(),
        goto_next_sibling(),
        goto_parent(),
    }
}
