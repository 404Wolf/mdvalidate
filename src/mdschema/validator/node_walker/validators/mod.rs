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
        node_walker::utils::pretty_print_cursor_pair,
        ts_utils::{parse_markdown, walk_to_root},
        validator_walker::ValidatorWalker,
    };

    use super::*;

    pub struct ValidatorTester<'a, V: Validator> {
        _phantom: std::marker::PhantomData<V>,
        input_tree: Tree,
        schema_tree: Tree,
        input_str: &'a str,
        schema_str: &'a str,
    }

    impl<'a, V: Validator> ValidatorTester<'a, V> {
        pub fn from_strs(schema_str: &'a str, input_str: &'a str) -> Self {
            let schema_tree = parse_markdown(schema_str).unwrap();
            let input_tree = parse_markdown(input_str).unwrap();

            Self {
                _phantom: std::marker::PhantomData,
                input_tree,
                schema_tree,
                input_str,
                schema_str,
            }
        }

        pub fn walk(&'_ self) -> ValidationTesterWalker<'_, V> {
            let input_cursor = self.input_tree.walk();
            let schema_cursor = self.schema_tree.walk();

            ValidationTesterWalker {
                _phantom: std::marker::PhantomData,
                input_cursor,
                schema_cursor,
                input_str: self.input_str,
                schema_str: self.schema_str,
            }
        }
    }

    pub struct ValidationTesterWalker<'a, V: Validator> {
        _phantom: std::marker::PhantomData<V>,
        input_cursor: TreeCursor<'a>,
        schema_cursor: TreeCursor<'a>,
        input_str: &'a str,
        schema_str: &'a str,
    }

    impl<'a, V: Validator> ValidationTesterWalker<'a, V> {
        pub fn validate(&mut self, got_eof: bool) -> ValidationResult {
            self.print();

            let walker = ValidatorWalker::from_cursors(
                &self.input_cursor,
                &self.schema_cursor,
                self.schema_str,
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

        /// Peek at the nodes that our cursors are currently positioned at.
        ///
        /// Calls your callback with the (input_node, schema_node).
        pub fn peek_nodes<F>(&mut self, f: F) -> &mut Self
        where
            F: Fn((&Node, &Node)),
        {
            f((&self.input_cursor.node(), &self.schema_cursor.node()));
            self
        }

        pub fn print(&mut self) -> &mut Self {
            let mut input_cursor = self.input_cursor.clone();
            walk_to_root(&mut input_cursor);

            let mut schema_cursor = self.schema_cursor.clone();
            walk_to_root(&mut schema_cursor);

            println!(
                "{}",
                pretty_print_cursor_pair(&input_cursor, &schema_cursor)
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
                            (self.input_cursor.$goto($($arg),*) && self.schema_cursor.$goto($($arg),*))
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
