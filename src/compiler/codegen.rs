use inkwell::{
    builder::Builder,
    context::Context,
    module::Module,
    types::{FloatType, IntType, PointerType},
    values::{BasicValueEnum, FloatValue, FunctionValue, IntValue, PointerValue},
    AddressSpace, FloatPredicate, IntPredicate,
};

use crate::{
    compiler::{Function, Scope, Value},
    BinaryOp, IdentifierOp, Node, Type, TypeLiteral, UnaryOp,
};

pub struct Codegen<'a, 'ctx> {
    pub context: &'ctx Context,
    pub module: &'a Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub function: FunctionValue<'ctx>,
    pub scope: Scope<'a, 'ctx>,

    pub int_type: IntType<'ctx>,
    pub float_type: FloatType<'ctx>,
    pub bool_type: IntType<'ctx>,
    pub char_type: IntType<'ctx>,
    pub str_type: PointerType<'ctx>,
}

impl<'a, 'ctx> Codegen<'a, 'ctx> {
    pub fn new(
        filename: &str,
        context: &'ctx Context,
        module: &'a Module<'ctx>,
        builder: Builder<'ctx>,
    ) -> Self {
        module.set_source_file_name(filename);

        let int_type = context.i32_type();
        let str_type = context.i8_type().ptr_type(AddressSpace::Generic);

        let fn_type = int_type.fn_type(&[int_type.into(), str_type.into()], false);
        let function = module.add_function("main", fn_type, None);
        let block = context.append_basic_block(function, "body");
        builder.position_at_end(block);

        let mut codegen = Self {
            context: &context,
            module: &module,
            builder,
            function,
            scope: Scope::new(None),

            int_type,
            float_type: context.f64_type(),
            bool_type: context.bool_type(),
            char_type: context.i8_type(),
            str_type,
        };
        codegen.print();
        codegen.math();
        codegen
    }

    pub fn create_child(&'a self, function: FunctionValue<'ctx>) -> Self {
        Self {
            context: self.context,
            module: self.module,
            builder: self.context.create_builder(),
            function,
            scope: Scope::new(Some(&self.scope)),

            int_type: self.int_type,
            float_type: self.float_type,
            bool_type: self.bool_type,
            char_type: self.char_type,
            str_type: self.str_type,
        }
    }

    pub fn generate_llvm_ir(&mut self, ast: Node) {
        self.visit(ast);
        self.builder.build_return(Some(&self.int_type.const_zero()));
    }

    pub fn add_var(&mut self, name: &str, value: Value<'ctx>) {
        self.scope
            .set(name.to_string(), value, &self.context, &self.builder);
    }

    fn visit(&mut self, node: Node) -> Value<'ctx> {
        match node {
            Node::Int(value) => Value::Int(self.int_type.const_int(value as u64, true)),
            Node::Float(value) => Value::Float(self.float_type.const_float(value)),
            Node::Bool(value) => {
                Value::Bool(self.bool_type.const_int(if value { 1 } else { 0 }, false))
            }
            Node::Str(value) => Value::Str({
                let string = self.context.const_string(value.as_bytes(), true);
                let ptr = self.builder.build_alloca(string.get_type(), "str");
                self.builder.build_store(ptr, string);
                ptr
            }),
            Node::Char(value) => Value::Char(self.char_type.const_int(value as u64, false)),
            Node::Array(nodes) => {
                let size = nodes.len() as u32;
                let mut ty = TypeLiteral::Int;

                let mut values: Vec<BasicValueEnum<'ctx>> = vec![];
                for node in nodes {
                    let value = self.visit(node);
                    ty = match value {
                        Value::Int(_) => TypeLiteral::Int,
                        Value::Float(_) => TypeLiteral::Float,
                        Value::Bool(_) => TypeLiteral::Bool,
                        Value::Str(_) => TypeLiteral::Str,
                        Value::Char(_) => TypeLiteral::Char,
                        _ => panic!("invalid array type"),
                    };
                    values.push(value.get_value());
                }

                let array = match ty {
                    TypeLiteral::Int => self.int_type.const_array(
                        values
                            .iter()
                            .map(|value| value.into_int_value())
                            .collect::<Vec<IntValue<'ctx>>>()
                            .as_slice(),
                    ),
                    TypeLiteral::Float => self.float_type.const_array(
                        values
                            .iter()
                            .map(|value| value.into_float_value())
                            .collect::<Vec<FloatValue<'ctx>>>()
                            .as_slice(),
                    ),
                    TypeLiteral::Bool => self.bool_type.const_array(
                        values
                            .iter()
                            .map(|value| value.into_int_value())
                            .collect::<Vec<IntValue<'ctx>>>()
                            .as_slice(),
                    ),
                    TypeLiteral::Str => self.str_type.const_array(
                        values
                            .iter()
                            .map(|value| value.into_pointer_value())
                            .collect::<Vec<PointerValue<'ctx>>>()
                            .as_slice(),
                    ),
                    TypeLiteral::Char => self.char_type.const_array(
                        values
                            .iter()
                            .map(|value| value.into_int_value())
                            .collect::<Vec<IntValue<'ctx>>>()
                            .as_slice(),
                    ),
                    TypeLiteral::Void => unreachable!(),
                };

                let size_value = self.int_type.const_int(size as u64, false);
                let ptr = match ty {
                    TypeLiteral::Int => {
                        self.builder
                            .build_array_alloca(self.int_type, size_value, "array")
                    }
                    TypeLiteral::Float => {
                        self.builder
                            .build_array_alloca(self.float_type, size_value, "array")
                    }
                    TypeLiteral::Bool => {
                        self.builder
                            .build_array_alloca(self.bool_type, size_value, "array")
                    }
                    TypeLiteral::Str => {
                        self.builder
                            .build_array_alloca(self.str_type, size_value, "array")
                    }
                    TypeLiteral::Char => {
                        self.builder
                            .build_array_alloca(self.char_type, size_value, "array")
                    }
                    TypeLiteral::Void => unreachable!(),
                };
                self.builder.build_store(ptr, array);

                Value::Array(ptr, ty, size)
            }
            Node::Cast(ty, node) => {
                let value = self.visit(*node);

                match ty {
                    Type::Int => Value::Int(match value {
                        Value::Int(value) | Value::Bool(value) => value,
                        Value::Float(value) => {
                            self.builder
                                .build_float_to_signed_int(value, self.int_type, "int")
                        }
                        _ => unimplemented!(),
                    }),
                    Type::Float => Value::Float(match value {
                        Value::Int(value) => {
                            self.builder
                                .build_signed_int_to_float(value, self.float_type, "float")
                        }
                        Value::Float(value) => value,
                        Value::Bool(value) => self.builder.build_unsigned_int_to_float(
                            value,
                            self.float_type,
                            "float",
                        ),
                        _ => unimplemented!(),
                    }),
                    Type::Bool => Value::Bool(match value {
                        Value::Int(value) | Value::Bool(value) => value,
                        Value::Float(value) => {
                            self.builder
                                .build_float_to_unsigned_int(value, self.bool_type, "bool")
                        }
                        _ => unimplemented!(),
                    }),
                    Type::Str => Value::Str(match value {
                        Value::Str(value) => value,
                        _ => unimplemented!(),
                    }),
                    Type::Char => Value::Char(match value {
                        Value::Char(value) => value,
                        _ => unimplemented!(),
                    }),
                    Type::Array(_, _) => panic!("can't cast to an array"),
                    Type::Void => panic!("can't cast to a void type"),
                }
            }
            Node::Identifier(name) => self.scope.get(&name, &self.builder),
            Node::Unary(op, node) => {
                let value = self.visit(*node);

                use UnaryOp::*;
                match op {
                    Pos => value,
                    Neg => match value {
                        Value::Int(value) => Value::Int(value.const_neg()),
                        Value::Float(value) => Value::Float(value.const_neg()),
                        _ => unimplemented!(),
                    },
                    Not => match value {
                        Value::Int(value) => Value::Bool(self.builder.build_int_compare(
                            IntPredicate::EQ,
                            value,
                            self.int_type.const_zero(),
                            "not",
                        )),
                        Value::Float(value) => Value::Bool(self.builder.build_float_compare(
                            FloatPredicate::OEQ,
                            value,
                            self.float_type.const_zero(),
                            "not",
                        )),
                        Value::Bool(value) => Value::Bool(self.builder.build_int_compare(
                            IntPredicate::EQ,
                            value,
                            self.bool_type.const_zero(),
                            "not",
                        )),
                        _ => unimplemented!(),
                    },
                }
            }
            Node::Binary(left, op, right) => {
                let l_value = self.visit(*left);
                let r_value = self.visit(*right);

                let f64_type = self.float_type;

                use BinaryOp::*;
                match op {
                    Add => match l_value {
                        Value::Int(l) => match r_value {
                            Value::Int(r) => Value::Int(self.builder.build_int_add(l, r, "add")),
                            Value::Float(r) => Value::Float(self.builder.build_float_add(
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "add",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Float(self.builder.build_float_add(
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "add",
                            )),
                            Value::Float(r) => {
                                Value::Float(self.builder.build_float_add(l, r, "add"))
                            }
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Sub => match l_value {
                        Value::Int(l) => match r_value {
                            Value::Int(r) => Value::Int(self.builder.build_int_sub(l, r, "sub")),
                            Value::Float(r) => Value::Float(self.builder.build_float_sub(
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "sub",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Float(self.builder.build_float_sub(
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "sub",
                            )),
                            Value::Float(r) => {
                                Value::Float(self.builder.build_float_sub(l, r, "sub"))
                            }
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Mul => match l_value {
                        Value::Int(l) => match r_value {
                            Value::Int(r) => Value::Int(self.builder.build_int_mul(l, r, "mul")),
                            Value::Float(r) => Value::Float(self.builder.build_float_mul(
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "mul",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Float(self.builder.build_float_mul(
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "mul",
                            )),
                            Value::Float(r) => {
                                Value::Float(self.builder.build_float_mul(l, r, "mul"))
                            }
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Div => match l_value {
                        Value::Int(l) => match r_value {
                            Value::Int(r) => {
                                Value::Int(self.builder.build_int_unsigned_div(l, r, "div"))
                            }
                            Value::Float(r) => Value::Float(self.builder.build_float_div(
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "div",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Float(self.builder.build_float_div(
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "div",
                            )),
                            Value::Float(r) => {
                                Value::Float(self.builder.build_float_div(l, r, "div"))
                            }
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Rem => match l_value {
                        Value::Int(l) => match r_value {
                            Value::Int(r) => {
                                Value::Int(self.builder.build_int_unsigned_rem(l, r, "rem"))
                            }
                            Value::Float(r) => Value::Float(self.builder.build_float_rem(
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "rem",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Float(self.builder.build_float_rem(
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "rem",
                            )),
                            Value::Float(r) => {
                                Value::Float(self.builder.build_float_rem(l, r, "rem"))
                            }
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    And => match l_value {
                        Value::Bool(l) => match r_value {
                            Value::Bool(r) => Value::Bool(self.builder.build_and(l, r, "and")),
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Or => match l_value {
                        Value::Bool(l) => match r_value {
                            Value::Bool(r) => Value::Bool(self.builder.build_or(l, r, "or")),
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    EqEq => match l_value {
                        Value::Int(l) | Value::Char(l) => match r_value {
                            Value::Int(r) | Value::Char(r) => Value::Bool(
                                self.builder
                                    .build_int_compare(IntPredicate::EQ, l, r, "eqeq"),
                            ),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OEQ,
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "eqeq",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OEQ,
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "eqeq",
                            )),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OEQ,
                                l,
                                r,
                                "eqeq",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Bool(l) => match r_value {
                            Value::Bool(r) => Value::Bool(
                                self.builder
                                    .build_not(self.builder.build_xor(l, r, "xor"), "not"),
                            ),
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Neq => match l_value {
                        Value::Int(l) | Value::Char(l) => match r_value {
                            Value::Int(r) | Value::Char(r) => Value::Bool(
                                self.builder
                                    .build_int_compare(IntPredicate::NE, l, r, "neq"),
                            ),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::ONE,
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "neq",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::ONE,
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "neq",
                            )),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::ONE,
                                l,
                                r,
                                "neq",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Bool(l) => match r_value {
                            Value::Bool(r) => Value::Bool(self.builder.build_xor(l, r, "xor")),
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Lt => match l_value {
                        Value::Int(l) | Value::Char(l) => match r_value {
                            Value::Int(r) | Value::Char(r) => Value::Bool(
                                self.builder
                                    .build_int_compare(IntPredicate::SLT, l, r, "lt"),
                            ),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OLT,
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "lt",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OLT,
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "lt",
                            )),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OLT,
                                l,
                                r,
                                "lt",
                            )),
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Lte => match l_value {
                        Value::Int(l) | Value::Char(l) => match r_value {
                            Value::Int(r) | Value::Char(r) => Value::Bool(
                                self.builder
                                    .build_int_compare(IntPredicate::SLE, l, r, "lte"),
                            ),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OLE,
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "lte",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OLE,
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "lte",
                            )),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OLE,
                                l,
                                r,
                                "lte",
                            )),
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Gt => match l_value {
                        Value::Int(l) | Value::Char(l) => match r_value {
                            Value::Int(r) | Value::Char(r) => Value::Bool(
                                self.builder
                                    .build_int_compare(IntPredicate::SGT, l, r, "gt"),
                            ),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OGT,
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "gt",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OGT,
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "gt",
                            )),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OGT,
                                l,
                                r,
                                "gt",
                            )),
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                    Gte => match l_value {
                        Value::Int(l) | Value::Char(l) => match r_value {
                            Value::Int(r) | Value::Char(r) => Value::Bool(
                                self.builder
                                    .build_int_compare(IntPredicate::SGE, l, r, "gte"),
                            ),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OGE,
                                self.builder.build_signed_int_to_float(l, f64_type, "left"),
                                r,
                                "gte",
                            )),
                            _ => unimplemented!(),
                        },
                        Value::Float(l) => match r_value {
                            Value::Int(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OGE,
                                l,
                                self.builder.build_signed_int_to_float(r, f64_type, "right"),
                                "gte",
                            )),
                            Value::Float(r) => Value::Bool(self.builder.build_float_compare(
                                FloatPredicate::OGE,
                                l,
                                r,
                                "gte",
                            )),
                            _ => unimplemented!(),
                        },
                        _ => unimplemented!(),
                    },
                }
            }
            Node::Let(name, node) => {
                let value = self.visit(*node);
                self.scope.set(name, value, &self.context, &self.builder)
            }
            Node::IdentifierOp(name, op, node) => {
                let ptr = match *name.clone() {
                    Node::Identifier(name) => self.scope.get_ptr(&name, &self.builder),
                    Node::Index(name, index) => match *name {
                        Node::Identifier(name) => {
                            let list_ptr = self.scope.get_ptr(&name, &self.builder);
                            let index = self.visit(*index);
                            unsafe {
                                self.builder.build_gep(
                                    list_ptr,
                                    &[match index {
                                        Value::Int(value) => value,
                                        _ => unimplemented!(),
                                    }],
                                    "index",
                                )
                            }
                        }
                        _ => unimplemented!(),
                    },
                    _ => unimplemented!(),
                };
                let value = self.visit(*node.clone());

                use IdentifierOp::*;

                macro_rules! identifier_op {
                    ($($op:tt),*) => {
                        match op {
                            Eq=>{
                                self.builder.build_store(ptr,value.get_value());
                                value
                            },
                            $(
                                $op => self.visit(Node::IdentifierOp(
                                    name.clone(),
                                    $op,
                                    Box::new(Node::Binary(
                                        name,
                                        BinaryOp::$op,
                                        node,
                                    )),
                                )),
                            )*
                        }
                    };
                }

                identifier_op!(Add, Sub, Mul, Div, Rem)
            }
            Node::Index(node, index) => {
                let value = self.visit(*node.clone());
                let index = self.visit(*index);
                match value {
                    Value::Str(ptr) => {
                        let index_ptr = unsafe {
                            self.builder.build_gep(
                                ptr,
                                &[match index {
                                    Value::Int(value) => value,
                                    _ => unimplemented!(),
                                }],
                                "index",
                            )
                        };
                        let item_value = self.builder.build_load(index_ptr, "index");
                        Value::Char(item_value.into_int_value())
                    }
                    Value::Array(ptr, ty, _) => {
                        let index_ptr = unsafe {
                            self.builder.build_gep(
                                ptr,
                                &[match index {
                                    Value::Int(value) => value,
                                    _ => unimplemented!(),
                                }],
                                "index",
                            )
                        };
                        let item_value = self.builder.build_load(index_ptr, "index");
                        match ty {
                            TypeLiteral::Int => Value::Int(item_value.into_int_value()),
                            TypeLiteral::Float => Value::Float(item_value.into_float_value()),
                            TypeLiteral::Bool => Value::Bool(item_value.into_int_value()),
                            TypeLiteral::Str => Value::Str(item_value.into_pointer_value()),
                            TypeLiteral::Char => Value::Char(item_value.into_int_value()),
                            TypeLiteral::Void => panic!("can't have a void array"),
                        }
                    }
                    _ => panic!("cannot index {}", node),
                }
            }
            Node::While(condition, body) => {
                let condition_block = self.context.append_basic_block(self.function, "while_cond");
                self.builder.build_unconditional_branch(condition_block);
                self.builder.position_at_end(condition_block);
                let condition_value = match self.visit(*condition) {
                    Value::Bool(value) => value,
                    _ => panic!("while loops can only have a bool as their condition"),
                };

                let loop_block = self.context.append_basic_block(self.function, "while_loop");
                self.builder.position_at_end(loop_block);
                self.visit(*body);

                let end_block = self.context.append_basic_block(self.function, "while_end");
                self.builder.build_unconditional_branch(condition_block);

                self.builder.position_at_end(condition_block);
                self.builder
                    .build_conditional_branch(condition_value, loop_block, end_block);

                self.builder.position_at_end(end_block);

                Value::Int(self.int_type.const_zero())
            }
            Node::If(condition, body, else_case) => {
                let condition_value = match self.visit(*condition) {
                    Value::Bool(value) => value,
                    _ => panic!("if statements can only have a bool as their condition"),
                };

                let then_block = self.context.append_basic_block(self.function, "then");
                match else_case {
                    Some(else_case) => {
                        let else_block = self.context.append_basic_block(self.function, "else");
                        let end_block = self.context.append_basic_block(self.function, "if_end");

                        self.builder.build_conditional_branch(
                            condition_value,
                            then_block,
                            else_block,
                        );

                        // Then
                        self.builder.position_at_end(then_block);
                        let then_value = self.visit(*body);
                        self.builder.build_unconditional_branch(end_block);

                        let then_block = self.builder.get_insert_block().unwrap();

                        // Else
                        self.builder.position_at_end(else_block);
                        let else_value = self.visit(*else_case);
                        self.builder.build_unconditional_branch(end_block);

                        let else_block = self.builder.get_insert_block().unwrap();

                        self.builder.position_at_end(end_block);

                        let phi = self
                            .builder
                            .build_phi(then_value.get_type(self.context), "phi");
                        phi.add_incoming(&[
                            (&then_value.get_value(), then_block),
                            (&else_value.get_value(), else_block),
                        ]);

                        let phi_value = phi.as_basic_value();
                        match then_value {
                            Value::Int(_) => Value::Int(phi_value.into_int_value()),
                            Value::Float(_) => Value::Float(phi_value.into_float_value()),
                            Value::Bool(_) => Value::Bool(phi_value.into_int_value()),
                            Value::Str(_) => Value::Str(phi_value.into_pointer_value()),
                            Value::Char(_) => Value::Char(phi_value.into_int_value()),
                            Value::Array(_, ty, size) => {
                                Value::Array(phi_value.into_pointer_value(), ty, size)
                            }
                            Value::Void => panic!("void isn't a valid type"),
                        }
                    }
                    None => {
                        let end_block = self.context.append_basic_block(self.function, "end");

                        self.builder.build_conditional_branch(
                            condition_value,
                            then_block,
                            end_block,
                        );

                        // Then
                        self.builder.position_at_end(then_block);
                        self.visit(*body);
                        self.builder.build_unconditional_branch(end_block);

                        self.builder.position_at_end(end_block);

                        Value::Int(self.int_type.const_zero())
                    }
                }
            }
            Node::Fn(name, args, return_type, body) => {
                let arg_types = args.iter().map(|(_, ty)| ty.clone()).collect::<Vec<Type>>();

                let function = Function::new_user(&name, arg_types, return_type.clone(), self);
                let block = self.context.append_basic_block(function.value, "body");

                let mut codegen = self.create_child(self.function);
                codegen.builder.position_at_end(block);
                args.iter().enumerate().for_each(|(i, (arg_name, ty))| {
                    let arg_name = arg_name.clone();
                    let value = function.value.get_nth_param(i as u32).unwrap();
                    match ty {
                        Type::Int => {
                            let val_ptr = codegen.builder.build_alloca(self.int_type, &arg_name);
                            codegen
                                .scope
                                .variables
                                .insert(arg_name, (val_ptr, Type::Int));
                            codegen.builder.build_store(val_ptr, value.into_int_value());
                        }
                        Type::Float => {
                            let val_ptr = codegen.builder.build_alloca(self.float_type, &arg_name);
                            codegen
                                .scope
                                .variables
                                .insert(arg_name, (val_ptr, Type::Float));
                            codegen
                                .builder
                                .build_store(val_ptr, value.into_float_value());
                        }
                        Type::Bool => {
                            let val_ptr = codegen.builder.build_alloca(self.bool_type, &arg_name);
                            codegen
                                .scope
                                .variables
                                .insert(arg_name, (val_ptr, Type::Bool));
                            codegen.builder.build_store(val_ptr, value.into_int_value());
                        }
                        Type::Str => {
                            codegen
                                .scope
                                .variables
                                .insert(arg_name, (value.into_pointer_value(), Type::Str));
                        }
                        Type::Char => {
                            let val_ptr = codegen.builder.build_alloca(self.char_type, &arg_name);
                            codegen
                                .scope
                                .variables
                                .insert(arg_name, (val_ptr, Type::Char));
                            codegen.builder.build_store(val_ptr, value.into_int_value());
                        }
                        Type::Array(arr_ty, size) => {
                            codegen.scope.variables.insert(
                                arg_name,
                                (value.into_pointer_value(), Type::Array(*arr_ty, *size)),
                            );
                        }
                        Type::Void => panic!("void isn't a valid argument type"),
                    };
                });

                codegen.visit(*body);
                if return_type == Type::Void {
                    codegen.builder.build_return(None);
                }

                Value::Int(self.int_type.const_zero())
            }
            Node::Return(node) => {
                let value = self.visit(*node);
                self.builder.build_return(Some(match &value {
                    Value::Int(value) => value,
                    Value::Float(value) => value,
                    Value::Bool(value) => value,
                    Value::Str(value) => value,
                    Value::Char(value) => value,
                    Value::Array(value, _, _) => value,
                    Value::Void => panic!("void isn't a valid type"),
                }));
                Value::Int(self.int_type.const_zero())
            }
            Node::Call(name, args) => {
                let mut arg_values = args
                    .iter()
                    .map(|arg| self.visit(arg.clone()))
                    .collect::<Vec<Value<'ctx>>>();

                let function = self.scope.get_function(&name);
                if name == "print" {
                    arg_values.insert(
                        0,
                        Value::Str(
                            self.generate_printf_format_string(&arg_values)
                                .into_pointer_value(),
                        ),
                    );
                }

                let value = function.call(arg_values, &self.builder);
                match function.return_type {
                    Type::Int => Value::Int(value.into_int_value()),
                    Type::Float => Value::Float(value.into_float_value()),
                    Type::Bool => Value::Bool(value.into_int_value()),
                    Type::Str => Value::Str(value.into_pointer_value()),
                    Type::Char => Value::Char(value.into_int_value()),
                    Type::Array(ty, size) => Value::Array(value.into_pointer_value(), ty, size),
                    Type::Void => Value::Void,
                }
            }
            Node::Statements(nodes) => {
                let mut rtn_value = Value::Int(self.int_type.const_zero());
                for node in nodes {
                    rtn_value = self.visit(node);
                }
                rtn_value
            }
            _ => unimplemented!(),
        }
    }
}
