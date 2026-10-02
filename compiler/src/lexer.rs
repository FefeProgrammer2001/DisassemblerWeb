use crate::metadata::{CompileError, SourceSpan};
use crate::token::{Token, TokenKind};

pub struct Lexer<'a> {
    _source: std::marker::PhantomData<&'a str>,
    chars: Vec<(usize, char)>,
    cursor: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        let chars: Vec<(usize, char)> = source.char_indices().collect();
        Self {
            _source: std::marker::PhantomData,
            chars,
            cursor: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.cursor).map(|(_, c)| *c)
    }

    fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.cursor + offset).map(|(_, c)| *c)
    }

    fn advance(&mut self) -> Option<char> {
        if let Some((_, c)) = self.chars.get(self.cursor) {
            let ch = *c;
            self.cursor += 1;
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(ch)
        } else {
            None
        }
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), CompileError> {
        while let Some(c) = self.peek_char() {
            if c.is_ascii_whitespace() {
                self.advance();
            } else if c == '/' && self.peek_char_at(1) == Some('/') {
                // Line comment
                self.advance();
                self.advance();
                while let Some(ch) = self.peek_char() {
                    if ch == '\n' {
                        self.advance();
                        break;
                    }
                    self.advance();
                }
            } else if c == '/' && self.peek_char_at(1) == Some('*') {
                // Block comment
                let start_line = self.line;
                let start_col = self.col;
                self.advance();
                self.advance();
                let mut closed = false;
                while let Some(ch) = self.peek_char() {
                    if ch == '*' && self.peek_char_at(1) == Some('/') {
                        self.advance();
                        self.advance();
                        closed = true;
                        break;
                    }
                    self.advance();
                }
                if !closed {
                    return Err(CompileError {
                        message: "Unterminated block comment /* ... */".to_string(),
                        line: start_line,
                        column: start_col,
                    });
                }
            } else {
                break;
            }
        }
        Ok(())
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, CompileError> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace_and_comments()?;
            let start_line = self.line;
            let start_col = self.col;

            let ch = match self.peek_char() {
                Some(c) => c,
                None => {
                    tokens.push(Token::new(
                        TokenKind::Eof,
                        SourceSpan {
                            start_line,
                            start_col,
                            end_line: start_line,
                            end_col: start_col,
                        },
                    ));
                    break;
                }
            };

            // Identifiers or Keywords
            if ch.is_ascii_alphabetic() || ch == '_' {
                let mut ident = String::new();
                while let Some(c) = self.peek_char() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        ident.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }

                let kind = match ident.as_str() {
                    "int" => TokenKind::Int,
                    "char" => TokenKind::Char,
                    "short" => TokenKind::Short,
                    "long" => TokenKind::Long,
                    "void" => TokenKind::Void,
                    "return" => TokenKind::Return,
                    "if" => TokenKind::If,
                    "else" => TokenKind::Else,
                    "while" => TokenKind::While,
                    "for" => TokenKind::For,
                    "do" => TokenKind::Do,
                    "break" => TokenKind::Break,
                    "continue" => TokenKind::Continue,
                    "sizeof" => TokenKind::Sizeof,
                    "struct" => TokenKind::Struct,
                    _ => TokenKind::Ident(ident),
                };

                tokens.push(Token::new(
                    kind,
                    SourceSpan {
                        start_line,
                        start_col,
                        end_line: self.line,
                        end_col: self.col,
                    },
                ));
                continue;
            }

            // Numbers
            if ch.is_ascii_digit() {
                let mut num_str = String::new();
                let mut is_hex = false;

                if ch == '0' && (self.peek_char_at(1) == Some('x') || self.peek_char_at(1) == Some('X')) {
                    is_hex = true;
                    self.advance(); // '0'
                    self.advance(); // 'x'
                    while let Some(c) = self.peek_char() {
                        if c.is_ascii_hexdigit() {
                            num_str.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                } else {
                    while let Some(c) = self.peek_char() {
                        if c.is_ascii_digit() {
                            num_str.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }

                let value = if is_hex {
                    i64::from_str_radix(&num_str, 16).unwrap_or(0)
                } else {
                    num_str.parse::<i64>().unwrap_or(0)
                };

                tokens.push(Token::new(
                    TokenKind::Number(value),
                    SourceSpan {
                        start_line,
                        start_col,
                        end_line: self.line,
                        end_col: self.col,
                    },
                ));
                continue;
            }

            // Character literal
            if ch == '\'' {
                self.advance(); // skip opening quote
                let char_val = self.parse_char_literal(start_line, start_col)?;
                tokens.push(Token::new(
                    TokenKind::CharLiteral(char_val),
                    SourceSpan {
                        start_line,
                        start_col,
                        end_line: self.line,
                        end_col: self.col,
                    },
                ));
                continue;
            }

            // String literal
            if ch == '"' {
                self.advance(); // skip opening quote
                let str_bytes = self.parse_string_literal(start_line, start_col)?;
                tokens.push(Token::new(
                    TokenKind::StringLiteral(str_bytes),
                    SourceSpan {
                        start_line,
                        start_col,
                        end_line: self.line,
                        end_col: self.col,
                    },
                ));
                continue;
            }

            // Multi-char operators
            let next_ch = self.peek_char_at(1);
            let next2_ch = self.peek_char_at(2);

            if ch == '<' && next_ch == Some('<') && next2_ch == Some('=') {
                self.advance();
                self.advance();
                self.advance();
                tokens.push(Token::new(
                    TokenKind::ShiftLeftEqual,
                    SourceSpan { start_line, start_col, end_line: self.line, end_col: self.col },
                ));
                continue;
            }
            if ch == '>' && next_ch == Some('>') && next2_ch == Some('=') {
                self.advance();
                self.advance();
                self.advance();
                tokens.push(Token::new(
                    TokenKind::ShiftRightEqual,
                    SourceSpan { start_line, start_col, end_line: self.line, end_col: self.col },
                ));
                continue;
            }

            // 2-char operators
            let two_char = match (ch, next_ch) {
                ('=', Some('=')) => Some(TokenKind::EqualEqual),
                ('!', Some('=')) => Some(TokenKind::ExclamationEqual),
                ('<', Some('=')) => Some(TokenKind::LessEqual),
                ('>', Some('=')) => Some(TokenKind::GreaterEqual),
                ('<', Some('<')) => Some(TokenKind::ShiftLeft),
                ('>', Some('>')) => Some(TokenKind::ShiftRight),
                ('&', Some('&')) => Some(TokenKind::AmpAmp),
                ('|', Some('|')) => Some(TokenKind::PipePipe),
                ('+', Some('+')) => Some(TokenKind::PlusPlus),
                ('-', Some('-')) => Some(TokenKind::MinusMinus),
                ('+', Some('=')) => Some(TokenKind::PlusEqual),
                ('-', Some('=')) => Some(TokenKind::MinusEqual),
                ('*', Some('=')) => Some(TokenKind::StarEqual),
                ('/', Some('=')) => Some(TokenKind::SlashEqual),
                ('%', Some('=')) => Some(TokenKind::PercentEqual),
                ('&', Some('=')) => Some(TokenKind::AmpEqual),
                ('|', Some('=')) => Some(TokenKind::PipeEqual),
                ('^', Some('=')) => Some(TokenKind::CaretEqual),
                ('-', Some('>')) => Some(TokenKind::Arrow),
                _ => None,
            };

            if let Some(kind) = two_char {
                self.advance();
                self.advance();
                tokens.push(Token::new(
                    kind,
                    SourceSpan {
                        start_line,
                        start_col,
                        end_line: self.line,
                        end_col: self.col,
                    },
                ));
                continue;
            }

            // 1-char tokens
            let single_char = match ch {
                '+' => Some(TokenKind::Plus),
                '-' => Some(TokenKind::Minus),
                '*' => Some(TokenKind::Star),
                '/' => Some(TokenKind::Slash),
                '%' => Some(TokenKind::Percent),
                '&' => Some(TokenKind::Amp),
                '|' => Some(TokenKind::Pipe),
                '^' => Some(TokenKind::Caret),
                '~' => Some(TokenKind::Tilde),
                '!' => Some(TokenKind::Exclamation),
                '<' => Some(TokenKind::Less),
                '>' => Some(TokenKind::Greater),
                '=' => Some(TokenKind::Equal),
                '.' => Some(TokenKind::Dot),
                ',' => Some(TokenKind::Comma),
                ';' => Some(TokenKind::Semicolon),
                ':' => Some(TokenKind::Colon),
                '?' => Some(TokenKind::Question),
                '(' => Some(TokenKind::LParen),
                ')' => Some(TokenKind::RParen),
                '{' => Some(TokenKind::LBrace),
                '}' => Some(TokenKind::RBrace),
                '[' => Some(TokenKind::LBracket),
                ']' => Some(TokenKind::RBracket),
                _ => None,
            };

            if let Some(kind) = single_char {
                self.advance();
                tokens.push(Token::new(
                    kind,
                    SourceSpan {
                        start_line,
                        start_col,
                        end_line: self.line,
                        end_col: self.col,
                    },
                ));
                continue;
            }

            return Err(CompileError {
                message: format!("Unexpected character: '{}'", ch),
                line: start_line,
                column: start_col,
            });
        }

        Ok(tokens)
    }

    fn parse_char_literal(&mut self, start_line: usize, start_col: usize) -> Result<u8, CompileError> {
        let val = match self.advance() {
            Some('\\') => match self.advance() {
                Some('n') => b'\n',
                Some('t') => b'\t',
                Some('r') => b'\r',
                Some('0') => 0,
                Some('\\') => b'\\',
                Some('\'') => b'\'',
                Some('"') => b'"',
                Some(other) => other as u8,
                None => {
                    return Err(CompileError {
                        message: "Unexpected end of file in character literal escape".to_string(),
                        line: start_line,
                        column: start_col,
                    })
                }
            },
            Some('\'') => {
                return Err(CompileError {
                    message: "Empty character constant".to_string(),
                    line: start_line,
                    column: start_col,
                })
            }
            Some(c) => c as u8,
            None => {
                return Err(CompileError {
                    message: "Unexpected end of file in character literal".to_string(),
                    line: start_line,
                    column: start_col,
                })
            }
        };

        if self.advance() != Some('\'') {
            return Err(CompileError {
                message: "Unclosed character literal, expected closing quote".to_string(),
                line: start_line,
                column: start_col,
            });
        }

        Ok(val)
    }

    fn parse_string_literal(&mut self, start_line: usize, start_col: usize) -> Result<Vec<u8>, CompileError> {
        let mut bytes = Vec::new();

        loop {
            match self.advance() {
                Some('"') => break,
                Some('\\') => match self.advance() {
                    Some('n') => bytes.push(b'\n'),
                    Some('t') => bytes.push(b'\t'),
                    Some('r') => bytes.push(b'\r'),
                    Some('0') => bytes.push(0),
                    Some('\\') => bytes.push(b'\\'),
                    Some('"') => bytes.push(b'"'),
                    Some('\'') => bytes.push(b'\''),
                    Some(c) => bytes.push(c as u8),
                    None => {
                        return Err(CompileError {
                            message: "Unexpected end of file in string literal escape".to_string(),
                            line: start_line,
                            column: start_col,
                        })
                    }
                },
                Some(c) => bytes.push(c as u8),
                None => {
                    return Err(CompileError {
                        message: "Unclosed string literal".to_string(),
                        line: start_line,
                        column: start_col,
                    })
                }
            }
        }

        bytes.push(0); // Null terminator
        Ok(bytes)
    }
}
