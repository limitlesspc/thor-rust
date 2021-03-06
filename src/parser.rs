use crate::{BinaryOp, IdentifierOp, Node, Token, Type, TypeLiteral, UnaryOp};

pub struct Parser {
    tokens: Vec<Token>,
    index: usize,
    token: Token,
}

use Token::*;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            token: tokens[0].clone(),
            tokens,
            index: 0,
        }
    }

    fn advance(&mut self) {
        self.index += 1;
        let next = self.tokens.get(self.index);
        self.token = match next {
            Some(token) => token.clone(),
            _ => EOF,
        };
    }

    fn back(&mut self) {
        self.index -= 2;
        self.advance();
    }

    

    fn skip_newlines(&mut self) -> u32 {
        let mut newlines = 0u32;
        while self.token == Newline {
            self.advance();
            newlines += 1;
        }
        newlines
    }

    pub fn parse(&mut self) -> Node {
        self.statements()
    }

    fn statements(&mut self) -> Node {
        let mut statements: Vec<Node> = vec![];
        self.skip_newlines();

        statements.push(self.statement());

        let mut more_statements = true;

        loop {
            let newlines = self.skip_newlines();
            if newlines == 0 {
                more_statements = false;
            }

            if !more_statements || self.token == RBrace {
                break;
            }

            let statement = self.statement();
            if statement == Node::EOF {
                more_statements = false;
                continue;
            }
            statements.push(statement);
        }

        Node::Statements(statements)
    }

    pub fn statement(&mut self) -> Node {
        match self.token {
            Let => {
                self.advance();

                let name = match self.token.clone() {
                    Identifier(name) => name,
                    _ => panic!("Expected identifier"),
                };
                self.advance();

                if self.token != Eq {
                    panic!("Expected '='");
                }self.advance();

                Node::Let(name,Box::new(self.expr()))
            }
            Return => {
                self.advance();
                Node::Return(Box::new(self.expr()))
            }
            _ => self.expr(),
        }
    }

    fn expr(&mut self) -> Node {
        let expr = self.or_expr();

        macro_rules! expr {
            ($(($token:tt, $op:tt)),*) => {
                match self.token {
                    $(
                        $token => {
                            self.advance();
                            Node::IdentifierOp(Box::new(expr), IdentifierOp::$op, Box::new(self.or_expr()))
                        }
                    )*,
                    _ => expr,
                }
            };
        }

        expr!(
            (Eq, Eq),
            (AddEq, Add),
            (SubEq, Sub),
            (MulEq, Mul),
            (DivEq, Div),
            (RemEq, Rem)
        )
    }

    fn or_expr(&mut self) -> Node {
        let result = self.and_expr();

        match self.token {
            Or => {
                self.advance();
                Node::Binary(Box::new(result), BinaryOp::Or, Box::new(self.or_expr()))
            }
            _ => result,
        }
    }

    fn and_expr(&mut self) -> Node {
        let result = self.not_expr();

        match self.token {
            And => {
                self.advance();
                Node::Binary(Box::new(result), BinaryOp::And, Box::new(self.and_expr()))
            }
            _ => result,
        }
    }

    fn not_expr(&mut self) -> Node {
        match self.token {
            Not => {
                self.advance();
                Node::Unary(UnaryOp::Not, Box::new(self.not_expr()))
            }
            _ => self.comp_expr(),
        }
    }

    fn comp_expr(&mut self) -> Node {
        let result = self.arith_expr();

        macro_rules! comp_expr {
            ($($token:tt),*) => {
                match self.token {
                    $(
                        $token => {
                            self.advance();
                            Node::Binary(Box::new(result), BinaryOp::$token, Box::new(self.comp_expr()))
                        },
                    )*
                    _ => result,
                }
            };
        }

        comp_expr!(EqEq, Neq, Lt, Lte, Gt, Gte)
    }

    fn arith_expr(&mut self) -> Node {
        let result = self.term();

        match self.token {
            Add => {
                self.advance();
                Node::Binary(Box::new(result), BinaryOp::Add, Box::new(self.arith_expr()))
            }
            Sub => {
                self.advance();
                Node::Binary(Box::new(result), BinaryOp::Sub, Box::new(self.arith_expr()))
            }
            _ => result,
        }
    }

    fn term(&mut self) -> Node {
        let result = self.factor();

        match self.token {
            Mul => {
                self.advance();
                Node::Binary(Box::new(result), BinaryOp::Mul, Box::new(self.term()))
            }
            Div => {
                self.advance();
                Node::Binary(Box::new(result), BinaryOp::Div, Box::new(self.term()))
            }
            Rem => {
                self.advance();
                Node::Binary(Box::new(result), BinaryOp::Rem, Box::new(self.term()))
            }
            _ => result,
        }
    }

    fn factor(&mut self) -> Node {
        match self.token {
            Add => {
                self.advance();
                Node::Unary(UnaryOp::Pos, Box::new(self.factor()))
            }
            Sub => {
                self.advance();
                Node::Unary(UnaryOp::Neg, Box::new(self.factor()))
            }
            _ => self.call(),
        }
    }

    fn call(&mut self) -> Node {
        let result = self.atom();

        match self.token {
            LParen => {
                self.advance();

                match result {
                    Node::Identifier(name) => {
                        let args = self.list(RParen);
                        Node::Call(name, args)
                    }
                    Node::Type(literal) => {
                        let expr = self.expr();

                        if self.token != RParen {
                            panic!("expected ')'");
                        }
                        self.advance();

                        Node::Cast(literal, Box::new(expr))
                    }
                    _ => panic!("expected identifier or type"),
                }
            }
            _ => result,
        }
    }

    fn atom(&mut self) -> Node {
        let result= match self.token.clone() {
            Int(value) => {
                self.advance();
                Node::Int(value)
            }
            Float(value) => {
                self.advance();
                Node::Float(value)
            }
            Bool(value) => {
                self.advance();
                Node::Bool(value)
            }
            Str(value) => {
                self.advance();
                Node::Str(value)
            }
            Char(value) => {
                self.advance();
                Node::Char(value)
            }
            Ty(literal) => {
                self.advance();

                let array_size = match self.token {
                    LBracket => {
                        self.advance();

                        let size = match self.token {
                            Int(size)  => size,
                            _ => panic!("array size must be an int")
                        };
                        self.advance(); 

                        if self.token != RBracket {
                            panic!("expected ']'");
                        }
                        self.advance();

                        Some(size)
                    },
                    _=>None
                };

               Node::Type(match array_size {
                    Some(size) => Type::Array(literal,size),
                    None => match literal {
                        TypeLiteral::Int=>Type::Int,
                        TypeLiteral::Float=>Type::Float,
                        TypeLiteral::Bool=>Type::Bool,
                        TypeLiteral::Str=>Type::Str,
                        TypeLiteral::Char=>Type::Char,
                        TypeLiteral::Void=>Type::Void
                    }
                })
            }
            Identifier(name) => {
                self.advance();
                Node::Identifier(name)
            }
            LParen => {
                self.advance();
                let result = self.expr();

                if self.token != RParen {
                    panic!("expected ')'");
                }
                self.advance();

                result
            }
            LBracket => self.array_expr(),
            If => self.if_expr(),
            While => self.while_expr(),
            For => self.for_expr(),
            Fn => self.fn_expr(),
            EOF => Node::EOF,
            _ => panic!("expected int, float, bool, str, type, identifier, '(', 'if', 'while', 'for', or 'fn'"),
        };
        match self.token {
            LBracket => Node::Index(Box::new(result), Box::new(self.index())),
            _ => result,
        }
    }

    fn array_expr(&mut self) -> Node {
        if self.token != LBracket {
            panic!("expected '['");
        }
        self.advance();

        let nodes = self.list(RBracket);

        Node::Array(nodes)
    }

    fn if_expr(&mut self) -> Node {
        if self.token != If {
            panic!("expected 'if'");
        }
        self.advance();

        let condition = self.expr();

        let body = match self.token {
            Colon => {
                self.advance();
                self.statement()
            }
            LBrace => self.block(),
            _ => panic!("{}", "expected ':' or '{'"),
        };

        let mut else_case: Option<Box<Node>> = None;
        let newlines = self.skip_newlines();
        if self.token == Else {
            else_case = Some(Box::new(self.else_expr()));
        } else if newlines > 0 {
            self.back();
        }

        let node = Node::If(Box::new(condition), Box::new(body), else_case);
        node
    }

    fn else_expr(&mut self) -> Node {
        if self.token != Else {
            panic!("expected 'else'");
        }
        self.advance();

        match self.token {
            Colon => {
                self.advance();
                self.statement()
            }
            LBrace => self.block(),
            If => self.if_expr(),
            _ => panic!("{}", "expected ':', '{', or 'if'"),
        }
    }

    fn while_expr(&mut self) -> Node {
        if self.token != While {
            panic!("expected 'while'");
        }
        self.advance();

        let condition = self.expr();

        let body = match self.token {
            Colon => {
                self.advance();
                self.statement()
            }
            LBrace => self.block(),
            _ => panic!("{}", "expected ':' or '{'"),
        };

        Node::While(Box::new(condition), Box::new(body))
    }

    fn for_expr(&mut self) -> Node {
        if self.token != For {
            panic!("expected 'for'");
        }
        self.advance();

        let identifier = match &self.token {
            Identifier(name) => {
                let n = name.clone();
                self.advance();
                n
            }
            _ => panic!("expected identifier"),
        };

        if self.token != In {
            panic!("expected 'in'");
        }
        self.advance();

        let iterable = self.expr();

        let body = match self.token {
            Colon => {
                self.advance();
                self.statement()
            }
            LBrace => self.block(),
            _ => panic!("{}", "expected ':' or '{'"),
        };

        Node::For(identifier, Box::new(iterable), Box::new(body))
    }

    fn fn_expr(&mut self) -> Node {
        if self.token != Fn {
            panic!("expected 'fn'");
        }
        self.advance();

        let name = match &self.token {
            Identifier(name) => name.clone(),
            _ => panic!("expected identifier"),
        };
        self.advance();

        if self.token != LParen {
            panic!("expected '('");
        }
        self.advance();

        let mut args: Vec<(String, Type)> = vec![];

        while self.token != RParen {
            let name = match &self.token {
                Identifier(name) => name.clone(),
                _ => panic!("expected identifier"),
            };
            self.advance();

            if self.token != Colon {
                panic!("expected ':'");
            }
            self.advance();

            let ty = match self.atom() {
                Node::Type(ty) => ty,
                _ => panic!("expected a type"),
            };

            match &self.token {
                Comma => self.advance(),
                RParen => {}
                _ => panic!("expected ',' or ')'"),
            };

            args.push((name, ty));
        }

        if self.token != RParen {
            panic!("expected '{}'", RParen);
        }
        self.advance();

        let return_type = match self.token {
            Colon => {
                self.advance();

                match self.atom() {
                    Node::Type(ty) => ty,
                    _ => panic!("expected type"),
                }
            }
            _ => Type::Void,
        };

        let body = match self.token {
            LBrace => self.block(),
            _ => panic!("{}", "expected '{'"),
        };

        Node::Fn(name.to_string(), args, return_type, Box::new(body))
    }

    fn list(&mut self, end: Token) -> Vec<Node> {
        let mut nodes: Vec<Node> = vec![];

        while self.token != end {
            nodes.push(self.expr());
            match &self.token {
                Comma => self.advance(),
                t if *t == end => {}
                _ => panic!("expected ',' or '{}'", end),
            };
        }

        if self.token != end {
            panic!("expected '{}'", end);
        }
        self.advance();

        nodes
    }

    fn block(&mut self) -> Node {
        if self.token != LBrace {
            panic!("{}", "expected '{'");
        }
        self.advance();

        let statements = self.statements();

        if self.token != RBrace {
            panic!("{}", "expected '}'");
        }
        self.advance();

        statements
    }

    fn index(&mut self) -> Node {
        if self.token != LBracket {
            panic!("{}", "expected '{'");
        }
        self.advance();

        let node = self.expr();

        if self.token != RBracket {
            panic!("expected ']'");
        }
        self.advance();

        node
    }
}
