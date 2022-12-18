/*
 * Created on Tue Sep 13 2022
 *
 * This file is a part of Skytable
 * Skytable (formerly known as TerrabaseDB or Skybase) is a free and open-source
 * NoSQL database written by Sayan Nandan ("the Author") with the
 * vision to provide flexibility in data modelling without compromising
 * on performance, queryability or scalability.
 *
 * Copyright (c) 2022, Sayan Nandan <ohsayan@outlook.com>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 *
*/

use std::ops::BitOr;

use {
    super::{LangError, LangResult, RawSlice},
    crate::util::{compiler, Life},
    core::{cmp, fmt, marker::PhantomData, mem::size_of, slice, str},
};

/*
    Lex meta
*/

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    Symbol(Symbol),
    Keyword(Keyword),
    Ident(RawSlice),
    #[cfg(test)]
    /// A comma that can be ignored (used for fuzzing)
    IgnorableComma,
    Lit(Lit), // literal
}

impl PartialEq<Symbol> for Token {
    fn eq(&self, other: &Symbol) -> bool {
        match self {
            Self::Symbol(s) => s == other,
            _ => false,
        }
    }
}

assertions! {
    size_of::<Token>() == 24, // FIXME(@ohsayan): Damn, what?
    size_of::<Symbol>() == 1,
    size_of::<Keyword>() == 1,
    size_of::<Lit>() == 24, // FIXME(@ohsayan): Ouch
}

enum_impls! {
    Token => {
        Keyword as Keyword,
        Symbol as Symbol,
        Lit as Lit,
    }
}

#[derive(Debug, PartialEq, Clone)]
#[repr(u8)]
pub enum Lit {
    Str(Box<str>),
    Bool(bool),
    UnsignedInt(u64),
    SignedInt(i64),
    Bin(RawSlice),
}

impl From<&'static str> for Lit {
    fn from(s: &'static str) -> Self {
        Self::Str(s.into())
    }
}

enum_impls! {
    Lit => {
        Box<str> as Str,
        String as Str,
        bool as Bool,
        u64 as UnsignedInt,
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Symbol {
    OpArithmeticAdd,  // +
    OpArithmeticSub,  // -
    OpArithmeticMul,  // *
    OpArithmeticDiv,  // /
    OpLogicalNot,     // !
    OpLogicalAnd,     // &
    OpLogicalXor,     // ^
    OpLogicalOr,      // |
    OpAssign,         // =
    TtOpenParen,      // (
    TtCloseParen,     // )
    TtOpenSqBracket,  // [
    TtCloseSqBracket, // ]
    TtOpenBrace,      // {
    TtCloseBrace,     // }
    OpComparatorLt,   // <
    OpComparatorGt,   // >
    QuoteS,           // '
    QuoteD,           // "
    SymAt,            // @
    SymHash,          // #
    SymDollar,        // $
    SymPercent,       // %
    SymUnderscore,    // _
    SymBackslash,     // \
    SymColon,         // :
    SymSemicolon,     // ;
    SymComma,         // ,
    SymPeriod,        // .
    SymQuestion,      // ?
    SymTilde,         // ~
    SymAccent,        // `
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u8)]
pub enum Keyword {
    Table,
    Model,
    Space,
    Index,
    Type,
    Function,
    Use,
    Create,
    Alter,
    Drop,
    Describe,
    Truncate,
    Rename,
    Add,
    Remove,
    Transform,
    Order,
    By,
    Primary,
    Key,
    Value,
    With,
    On,
    Lock,
    All,
    Insert,
    Select,
    Exists,
    Update,
    Delere,
    Into,
    From,
    As,
    Return,
    Sort,
    Group,
    Limit,
    Asc,
    Desc,
    To,
    Set,
    Auto,
    Default,
    In,
    Of,
    Transaction,
    Batch,
    Read,
    Write,
    Begin,
    End,
    Where,
    If,
    And,
    Or,
    Not,
    User,
    Revoke,
    Null,
    Infinity,
}

/*
    This section implements LUTs constructed using DAGs, as described by Czech et al in their paper. I wrote these pretty much by
    brute-force using a byte-level multiplicative function (inside a script). This unfortunately implies that every time we *do*
    need to add a new keyword, I will need to recompute and rewrite the vertices. I don't plan to use any codegen, so I think
    this is good as-is. The real challenge here is to keep the graph small, and I couldn't do that for the symbols table even with
    multiple trials. Please see if you can improve them.

    Also the functions are unique to every graph, and every input set, so BE WARNED!

    -- Sayan (@ohsayan)
    Sept. 18, 2022
*/

const SYM_MAGIC_A: u8 = b'w';
const SYM_MAGIC_B: u8 = b'E';

static SYM_GRAPH: [u8; 69] = [
    0, 0, 25, 0, 3, 0, 21, 0, 6, 13, 0, 0, 0, 0, 8, 0, 0, 0, 17, 0, 0, 30, 0, 28, 0, 20, 19, 12, 0,
    0, 2, 0, 0, 15, 0, 0, 0, 5, 0, 31, 14, 0, 1, 0, 18, 29, 24, 0, 0, 10, 0, 0, 26, 0, 0, 0, 22, 0,
    23, 7, 0, 27, 0, 4, 16, 11, 0, 0, 9,
];

static SYM_LUT: [(u8, Symbol); 32] = [
    (b'+', Symbol::OpArithmeticAdd),
    (b'-', Symbol::OpArithmeticSub),
    (b'*', Symbol::OpArithmeticMul),
    (b'/', Symbol::OpArithmeticDiv),
    (b'!', Symbol::OpLogicalNot),
    (b'&', Symbol::OpLogicalAnd),
    (b'^', Symbol::OpLogicalXor),
    (b'|', Symbol::OpLogicalOr),
    (b'=', Symbol::OpAssign),
    (b'(', Symbol::TtOpenParen),
    (b')', Symbol::TtCloseParen),
    (b'[', Symbol::TtOpenSqBracket),
    (b']', Symbol::TtCloseSqBracket),
    (b'{', Symbol::TtOpenBrace),
    (b'}', Symbol::TtCloseBrace),
    (b'<', Symbol::OpComparatorLt),
    (b'>', Symbol::OpComparatorGt),
    (b'\'', Symbol::QuoteS),
    (b'"', Symbol::QuoteD),
    (b'@', Symbol::SymAt),
    (b'#', Symbol::SymHash),
    (b'$', Symbol::SymDollar),
    (b'%', Symbol::SymPercent),
    (b'_', Symbol::SymUnderscore),
    (b'\\', Symbol::SymBackslash),
    (b':', Symbol::SymColon),
    (b';', Symbol::SymSemicolon),
    (b',', Symbol::SymComma),
    (b'.', Symbol::SymPeriod),
    (b'?', Symbol::SymQuestion),
    (b'~', Symbol::SymTilde),
    (b'`', Symbol::SymAccent),
];

#[inline(always)]
fn symfh(k: u8, magic: u8) -> u16 {
    (magic as u16 * k as u16) % SYM_GRAPH.len() as u16
}

#[inline(always)]
fn symph(k: u8) -> u8 {
    (SYM_GRAPH[symfh(k, SYM_MAGIC_A) as usize] + SYM_GRAPH[symfh(k, SYM_MAGIC_B) as usize])
        % SYM_GRAPH.len() as u8
}

#[inline(always)]
fn symof(sym: u8) -> Option<Symbol> {
    let hf = symph(sym);
    if hf < SYM_LUT.len() as u8 && SYM_LUT[hf as usize].0 == sym {
        Some(SYM_LUT[hf as usize].1)
    } else {
        None
    }
}

static KW_LUT: [(&[u8], Keyword); 60] = [
    (b"table", Keyword::Table),
    (b"model", Keyword::Model),
    (b"space", Keyword::Space),
    (b"index", Keyword::Index),
    (b"type", Keyword::Type),
    (b"function", Keyword::Function),
    (b"use", Keyword::Use),
    (b"create", Keyword::Create),
    (b"alter", Keyword::Alter),
    (b"drop", Keyword::Drop),
    (b"describe", Keyword::Describe),
    (b"truncate", Keyword::Truncate),
    (b"rename", Keyword::Rename),
    (b"add", Keyword::Add),
    (b"remove", Keyword::Remove),
    (b"transform", Keyword::Transform),
    (b"order", Keyword::Order),
    (b"by", Keyword::By),
    (b"primary", Keyword::Primary),
    (b"key", Keyword::Key),
    (b"value", Keyword::Value),
    (b"with", Keyword::With),
    (b"on", Keyword::On),
    (b"lock", Keyword::Lock),
    (b"all", Keyword::All),
    (b"insert", Keyword::Insert),
    (b"select", Keyword::Select),
    (b"exists", Keyword::Exists),
    (b"update", Keyword::Update),
    (b"delere", Keyword::Delere),
    (b"into", Keyword::Into),
    (b"from", Keyword::From),
    (b"as", Keyword::As),
    (b"return", Keyword::Return),
    (b"sort", Keyword::Sort),
    (b"group", Keyword::Group),
    (b"limit", Keyword::Limit),
    (b"asc", Keyword::Asc),
    (b"desc", Keyword::Desc),
    (b"to", Keyword::To),
    (b"set", Keyword::Set),
    (b"auto", Keyword::Auto),
    (b"default", Keyword::Default),
    (b"in", Keyword::In),
    (b"of", Keyword::Of),
    (b"transaction", Keyword::Transaction),
    (b"batch", Keyword::Batch),
    (b"read", Keyword::Read),
    (b"write", Keyword::Write),
    (b"begin", Keyword::Begin),
    (b"end", Keyword::End),
    (b"where", Keyword::Where),
    (b"if", Keyword::If),
    (b"and", Keyword::And),
    (b"or", Keyword::Or),
    (b"not", Keyword::Not),
    (b"user", Keyword::User),
    (b"revoke", Keyword::Revoke),
    (b"null", Keyword::Null),
    (b"infinity", Keyword::Infinity),
];

static KWG: [u8; 64] = [
    0, 55, 32, 25, 4, 21, 51, 43, 28, 59, 34, 1, 9, 39, 5, 49, 0, 16, 29, 0, 48, 0, 17, 60, 19, 21,
    26, 18, 0, 41, 55, 10, 48, 62, 55, 35, 56, 18, 29, 41, 5, 46, 25, 52, 32, 26, 27, 17, 61, 60,
    61, 59, 24, 12, 17, 30, 53, 4, 17, 0, 6, 2, 45, 56,
];

const KWMG_1: [u8; 11] = *b"nJEcjrLflKX";
const KWMG_2: [u8; 11] = *b"KWHPUPK3Fh3";
const KWMG_S: usize = KWMG_1.len();

#[inline(always)]
fn kwhf(k: &[u8], mg: &[u8]) -> u32 {
    let mut i = 0;
    let mut s = 0;
    while i < k.len() {
        s += mg[(i % KWMG_S) as usize] as u32 * k[i] as u32;
        i += 1;
    }
    s % KWG.len() as u32
}

#[inline(always)]
fn kwph(k: &[u8]) -> u8 {
    (KWG[kwhf(k, &KWMG_1) as usize] + KWG[kwhf(k, &KWMG_2) as usize]) % KWG.len() as u8
}

#[inline(always)]
fn kwof(key: &[u8]) -> Option<Keyword> {
    let ph = kwph(key);
    if ph < KW_LUT.len() as u8 && KW_LUT[ph as usize].0 == key {
        Some(KW_LUT[ph as usize].1)
    } else {
        None
    }
}

/*
    Lexer impl
*/

pub(super) const LANG_MODE_INSECURE: u8 = 0;
pub(super) const LANG_MODE_SECURE: u8 = 1;

pub type OperatingMode = u8;
pub type InsecureLexer<'a> = Lexer<'a, LANG_MODE_INSECURE>;
pub type SecureLexer<'a> = Lexer<'a, LANG_MODE_SECURE>;

pub struct Lexer<'a, const OPERATING_MODE: u8> {
    c: *const u8,
    e: *const u8,
    last_error: Option<LangError>,
    tokens: Vec<Token>,
    _lt: PhantomData<&'a [u8]>,
}

impl<'a, const OPERATING_MODE: u8> Lexer<'a, OPERATING_MODE> {
    #[inline(always)]
    pub const fn new(src: &'a [u8]) -> Self {
        Self {
            c: src.as_ptr(),
            e: unsafe {
                // UNSAFE(@ohsayan): Always safe (<= EOA)
                src.as_ptr().add(src.len())
            },
            last_error: None,
            tokens: Vec::new(),
            _lt: PhantomData,
        }
    }
}

// meta
impl<'a, const OPERATING_MODE: u8> Lexer<'a, OPERATING_MODE> {
    #[inline(always)]
    const fn cursor(&self) -> *const u8 {
        self.c
    }
    #[inline(always)]
    const fn data_end_ptr(&self) -> *const u8 {
        self.e
    }
    #[inline(always)]
    fn not_exhausted(&self) -> bool {
        self.data_end_ptr() > self.cursor()
    }
    #[inline(always)]
    fn exhausted(&self) -> bool {
        self.cursor() == self.data_end_ptr()
    }
    #[inline(always)]
    fn remaining(&self) -> usize {
        unsafe { self.e.offset_from(self.c) as usize }
    }
    #[inline(always)]
    unsafe fn deref_cursor(&self) -> u8 {
        *self.cursor()
    }
    #[inline(always)]
    unsafe fn incr_cursor_by(&mut self, by: usize) {
        debug_assert!(self.remaining() >= by);
        self.c = self.cursor().add(by)
    }
    #[inline(always)]
    unsafe fn incr_cursor(&mut self) {
        self.incr_cursor_by(1)
    }
    #[inline(always)]
    unsafe fn incr_cursor_if(&mut self, iff: bool) {
        self.incr_cursor_by(iff as usize)
    }
    #[inline(always)]
    fn push_token(&mut self, token: impl Into<Token>) {
        self.tokens.push(token.into())
    }
    #[inline(always)]
    fn peek_is(&mut self, f: impl FnOnce(u8) -> bool) -> bool {
        self.not_exhausted() && unsafe { f(self.deref_cursor()) }
    }
    #[inline(always)]
    fn peek_is_and_forward(&mut self, f: impl FnOnce(u8) -> bool) -> bool {
        let did_fw = self.not_exhausted() && unsafe { f(self.deref_cursor()) };
        unsafe {
            self.incr_cursor_if(did_fw);
        }
        did_fw
    }
    #[inline(always)]
    fn peek_eq_and_forward_or_eof(&mut self, eq: u8) -> bool {
        unsafe {
            let eq = self.not_exhausted() && self.deref_cursor() == eq;
            self.incr_cursor_if(eq);
            eq | self.exhausted()
        }
    }
    #[inline(always)]
    fn peek_neq(&self, b: u8) -> bool {
        self.not_exhausted() && unsafe { self.deref_cursor() != b }
    }
    #[inline(always)]
    fn peek_eq_and_forward(&mut self, b: u8) -> bool {
        unsafe {
            let r = self.not_exhausted() && self.deref_cursor() == b;
            self.incr_cursor_if(r);
            r
        }
    }
    #[inline(always)]
    fn trim_ahead(&mut self) {
        while self.peek_is_and_forward(|b| b == b' ' || b == b'\t' || b == b'\n') {}
    }
    #[inline(always)]
    fn set_error(&mut self, e: LangError) {
        self.last_error = Some(e);
    }
    #[inline(always)]
    fn no_error(&self) -> bool {
        self.last_error.is_none()
    }
}

impl<'a, const OPERATING_MODE: u8> Lexer<'a, OPERATING_MODE> {
    #[inline(always)]
    fn scan_ident(&mut self) -> RawSlice {
        let s = self.cursor();
        unsafe {
            while self.peek_is(|b| b.is_ascii_alphanumeric() || b == b'_') {
                self.incr_cursor();
            }
            RawSlice::new(s, self.cursor().offset_from(s) as usize)
        }
    }
    #[inline(always)]
    fn scan_ident_or_keyword(&mut self) {
        let s = self.scan_ident();
        let st = unsafe { s.as_slice() }.to_ascii_lowercase();
        match kwof(&st) {
            Some(kw) => self.tokens.push(kw.into()),
            // FIXME(@ohsayan): Uh, mind fixing this? The only advantage is that I can keep the graph *memory* footprint small
            None if st == b"true" || st == b"false" => self.push_token(Lit::Bool(st == b"true")),
            None => self.tokens.push(Token::Ident(s)),
        }
    }
    #[inline(always)]
    fn scan_unsigned_integer(&mut self) {
        let s = self.cursor();
        unsafe {
            while self.peek_is(|b| b.is_ascii_digit()) {
                self.incr_cursor();
            }
            /*
                1234; // valid
                1234} // valid
                1234{ // invalid
                1234, // valid
                1234a // invalid
            */
            let wseof = self.peek_is(|char| !char.is_ascii_alphabetic()) || self.exhausted();
            match str::from_utf8_unchecked(slice::from_raw_parts(
                s,
                self.cursor().offset_from(s) as usize,
            ))
            .parse()
            {
                Ok(num) if compiler::likely(wseof) => {
                    self.tokens.push(Token::Lit(Lit::UnsignedInt(num)))
                }
                _ => self.set_error(LangError::InvalidNumericLiteral),
            }
        }
    }
    #[inline(always)]
    fn scan_quoted_string(&mut self, quote_style: u8) {
        debug_assert!(
            unsafe { self.deref_cursor() } == quote_style,
            "illegal call to scan_quoted_string"
        );
        unsafe { self.incr_cursor() }
        let mut buf = Vec::new();
        unsafe {
            while self.peek_neq(quote_style) {
                match self.deref_cursor() {
                    b if b != b'\\' => {
                        buf.push(b);
                    }
                    _ => {
                        self.incr_cursor();
                        if self.exhausted() {
                            break;
                        }
                        let b = self.deref_cursor();
                        let quote = b == quote_style;
                        let bs = b == b'\\';
                        if quote | bs {
                            buf.push(b);
                        } else {
                            break; // what on good earth is that escape?
                        }
                    }
                }
                self.incr_cursor();
            }
            let terminated = self.peek_eq_and_forward(quote_style);
            match String::from_utf8(buf) {
                Ok(st) if terminated => self.tokens.push(Token::Lit(st.into_boxed_str().into())),
                _ => self.set_error(LangError::InvalidStringLiteral),
            }
        }
    }
    #[inline(always)]
    fn scan_byte(&mut self, byte: u8) {
        match symof(byte) {
            Some(tok) => self.push_token(tok),
            None => return self.set_error(LangError::UnexpectedChar),
        }
        unsafe {
            self.incr_cursor();
        }
    }
    #[inline(always)]
    fn scan_binary_literal(&mut self) {
        unsafe {
            self.incr_cursor();
        }
        let mut size = 0usize;
        let mut okay = true;
        while self.not_exhausted() && unsafe { self.deref_cursor() != b'\n' } && okay {
            /*
                Don't ask me how stupid this is. Like, I was probably in some "mood" when I wrote this
                and it works duh, but isn't the most elegant of things (could I have just used a parse?
                nah, I'm just a hardcore numeric normie)
                -- Sayan
            */
            let byte = unsafe { self.deref_cursor() };
            okay &= byte.is_ascii_digit();
            let (prod, of_flag) = size.overflowing_mul(10);
            okay &= !of_flag;
            let (sum, of_flag) = prod.overflowing_add((byte & 0x0F) as _);
            size = sum;
            okay &= !of_flag;
            unsafe {
                self.incr_cursor();
            }
        }
        okay &= self.peek_eq_and_forward(b'\n');
        okay &= self.remaining() >= size;
        if compiler::likely(okay) {
            unsafe {
                self.push_token(Lit::Bin(RawSlice::new(self.cursor(), size)));
                self.incr_cursor_by(size);
            }
        } else {
            self.set_error(LangError::InvalidSafeLiteral);
        }
    }
    #[inline(always)]
    fn scan_signed_integer(&mut self) {
        unsafe {
            self.incr_cursor();
        }
        if self.peek_is(|b| b.is_ascii_digit()) {
            // we have some digits
            let start = unsafe {
                // UNSAFE(@ohsayan): Take the (-) into the parse
                // TODO(@ohsayan): we can maybe look at a more efficient way later
                self.cursor().sub(1)
            };
            while self.peek_is_and_forward(|b| b.is_ascii_digit()) {}
            let wseof = self.peek_is(|char| !char.is_ascii_alphabetic()) || self.exhausted();
            match unsafe {
                str::from_utf8_unchecked(slice::from_raw_parts(
                    start,
                    self.cursor().offset_from(start) as usize,
                ))
            }
            .parse::<i64>()
            {
                Ok(num) if compiler::likely(wseof) => {
                    self.push_token(Lit::SignedInt(num));
                }
                _ => {
                    compiler::cold_val(self.set_error(LangError::InvalidNumericLiteral));
                }
            }
        } else {
            self.push_token(Token![-]);
        }
    }
    #[inline(always)]
    fn _lex(&mut self) {
        while self.not_exhausted() && self.no_error() {
            match unsafe { self.deref_cursor() } {
                // secure features
                byte if byte.is_ascii_alphabetic() => self.scan_ident_or_keyword(),
                #[cfg(test)]
                byte if byte == b'\x01' => {
                    self.push_token(Token::IgnorableComma);
                    unsafe {
                        // UNSAFE(@ohsayan): All good here. Already read the token
                        self.incr_cursor();
                    }
                }
                // insecure features
                byte if byte.is_ascii_digit() && OPERATING_MODE == LANG_MODE_INSECURE => {
                    self.scan_unsigned_integer()
                }
                b'-' if OPERATING_MODE == LANG_MODE_INSECURE => self.scan_signed_integer(),
                qs @ (b'\'' | b'"') if OPERATING_MODE == LANG_MODE_INSECURE => {
                    self.scan_quoted_string(qs)
                }
                // blank space or an arbitrary byte
                b' ' | b'\n' | b'\t' => self.trim_ahead(),
                b => self.scan_byte(b),
            }
        }
    }
    #[inline(always)]
    pub fn lex(src: &'a [u8]) -> LangResult<Life<'a, Vec<Token>>> {
        let mut slf = Self::new(src);
        slf._lex();
        match slf.last_error {
            None => Ok(Life::new(slf.tokens)),
            Some(e) => Err(e),
        }
    }
}

impl Token {
    #[inline(always)]
    pub(crate) const fn is_ident(&self) -> bool {
        matches!(self, Token::Ident(_))
    }
    #[inline(always)]
    pub(super) const fn is_lit(&self) -> bool {
        matches!(self, Self::Lit(_))
    }
}

impl AsRef<Token> for Token {
    #[inline(always)]
    fn as_ref(&self) -> &Token {
        self
    }
}

#[derive(Debug)]
pub struct RawLexer<'a> {
    c: *const u8,
    e: *const u8,
    tokens: Vec<Token>,
    last_error: Option<LangError>,
    _lt: PhantomData<&'a [u8]>,
}

// ctor
impl<'a> RawLexer<'a> {
    #[inline(always)]
    pub const fn new(src: &'a [u8]) -> Self {
        Self {
            c: src.as_ptr(),
            e: unsafe {
                // UNSAFE(@ohsayan): Always safe (<= EOA)
                src.as_ptr().add(src.len())
            },
            last_error: None,
            tokens: Vec::new(),
            _lt: PhantomData,
        }
    }
}

// meta
impl<'a> RawLexer<'a> {
    #[inline(always)]
    const fn cursor(&self) -> *const u8 {
        self.c
    }
    #[inline(always)]
    const fn data_end_ptr(&self) -> *const u8 {
        self.e
    }
    #[inline(always)]
    fn not_exhausted(&self) -> bool {
        self.data_end_ptr() > self.cursor()
    }
    #[inline(always)]
    fn exhausted(&self) -> bool {
        self.cursor() == self.data_end_ptr()
    }
    #[inline(always)]
    fn remaining(&self) -> usize {
        unsafe { self.e.offset_from(self.c) as usize }
    }
    #[inline(always)]
    unsafe fn deref_cursor(&self) -> u8 {
        *self.cursor()
    }
    #[inline(always)]
    unsafe fn incr_cursor_by(&mut self, by: usize) {
        debug_assert!(self.remaining() >= by);
        self.c = self.cursor().add(by)
    }
    #[inline(always)]
    unsafe fn incr_cursor(&mut self) {
        self.incr_cursor_by(1)
    }
    #[inline(always)]
    unsafe fn incr_cursor_if(&mut self, iff: bool) {
        self.incr_cursor_by(iff as usize)
    }
    #[inline(always)]
    fn push_token(&mut self, token: impl Into<Token>) {
        self.tokens.push(token.into())
    }
    #[inline(always)]
    fn peek_is(&mut self, f: impl FnOnce(u8) -> bool) -> bool {
        self.not_exhausted() && unsafe { f(self.deref_cursor()) }
    }
    #[inline(always)]
    fn peek_is_and_forward(&mut self, f: impl FnOnce(u8) -> bool) -> bool {
        let did_fw = self.not_exhausted() && unsafe { f(self.deref_cursor()) };
        unsafe {
            self.incr_cursor_if(did_fw);
        }
        did_fw
    }
    #[inline(always)]
    fn peek_eq_and_forward_or_eof(&mut self, eq: u8) -> bool {
        unsafe {
            let eq = self.not_exhausted() && self.deref_cursor() == eq;
            self.incr_cursor_if(eq);
            eq | self.exhausted()
        }
    }
    #[inline(always)]
    fn peek_neq(&self, b: u8) -> bool {
        self.not_exhausted() && unsafe { self.deref_cursor() != b }
    }
    #[inline(always)]
    fn peek_eq_and_forward(&mut self, b: u8) -> bool {
        unsafe {
            let r = self.not_exhausted() && self.deref_cursor() == b;
            self.incr_cursor_if(r);
            r
        }
    }
    #[inline(always)]
    fn trim_ahead(&mut self) {
        while self.peek_is_and_forward(|b| b == b' ' || b == b'\t' || b == b'\n') {}
    }
    #[inline(always)]
    fn set_error(&mut self, e: LangError) {
        self.last_error = Some(e);
    }
    #[inline(always)]
    fn no_error(&self) -> bool {
        self.last_error.is_none()
    }
}

// high level methods
impl<'a> RawLexer<'a> {
    #[inline(always)]
    fn scan_ident(&mut self) -> RawSlice {
        let s = self.cursor();
        unsafe {
            while self.peek_is(|b| b.is_ascii_alphanumeric() || b == b'_') {
                self.incr_cursor();
            }
            RawSlice::new(s, self.cursor().offset_from(s) as usize)
        }
    }
    #[inline(always)]
    fn scan_ident_or_keyword(&mut self) {
        let s = self.scan_ident();
        let st = unsafe { s.as_slice() }.to_ascii_lowercase();
        match kwof(&st) {
            Some(kw) => self.tokens.push(kw.into()),
            // FIXME(@ohsayan): Uh, mind fixing this? The only advantage is that I can keep the graph *memory* footprint small
            None if st == b"true" || st == b"false" => self.push_token(Lit::Bool(st == b"true")),
            None => self.tokens.push(Token::Ident(s)),
        }
    }
    #[inline(always)]
    fn scan_byte(&mut self, byte: u8) {
        match symof(byte) {
            Some(tok) => self.push_token(tok),
            None => return self.set_error(LangError::UnexpectedChar),
        }
        unsafe {
            self.incr_cursor();
        }
    }
}

#[derive(Debug)]
/// This lexer implements the `opmod-safe` for BlueQL
pub struct SafeLexer<'a> {
    base: RawLexer<'a>,
}

impl<'a> SafeLexer<'a> {
    #[inline(always)]
    pub const fn new(src: &'a [u8]) -> Self {
        Self {
            base: RawLexer::new(src),
        }
    }
    #[inline(always)]
    pub fn lex(src: &'a [u8]) -> LangResult<Vec<Token>> {
        Self::new(src)._lex()
    }
    #[inline(always)]
    fn _lex(self) -> LangResult<Vec<Token>> {
        let Self { base: mut l } = self;
        while l.not_exhausted() && l.no_error() {
            let b = unsafe { l.deref_cursor() };
            match b {
                // ident or kw
                b if b.is_ascii_alphabetic() => l.scan_ident_or_keyword(),
                // extra terminal chars
                b'\n' | b'\t' | b' ' => l.trim_ahead(),
                // arbitrary byte
                b => l.scan_byte(b),
            }
        }
        let RawLexer {
            last_error, tokens, ..
        } = l;
        match last_error {
            None => Ok(tokens),
            Some(e) => Err(e),
        }
    }
}

const ALLOW_UNSIGNED: bool = false;
const ALLOW_SIGNED: bool = true;

pub trait NumberDefinition: Sized + fmt::Debug + Copy + Clone + BitOr<Self, Output = Self> {
    const ALLOW_SIGNED: bool;
    fn overflowing_mul(&self, v: u8) -> (Self, bool);
    fn overflowing_add(&self, v: u8) -> (Self, bool);
    fn negate(&mut self);
    fn qualified_max_length() -> usize;
    fn zero() -> Self;
    fn b(self, b: bool) -> Self;
}

macro_rules! impl_number_def {
	($(
        $ty:ty {$supports_signed:ident, $qualified_max_length:expr}),* $(,)?
    ) => {
		$(impl NumberDefinition for $ty {
			const ALLOW_SIGNED: bool = $supports_signed;
            #[inline(always)] fn zero() -> Self { 0 }
            #[inline(always)] fn b(self, b: bool) -> Self { b as Self * self }
			#[inline(always)]
			fn overflowing_mul(&self, v: u8) -> ($ty, bool) { <$ty>::overflowing_mul(*self, v as $ty) }
			#[inline(always)]
			fn overflowing_add(&self, v: u8) -> ($ty, bool) { <$ty>::overflowing_add(*self, v as $ty) }
			#[inline(always)]
			fn negate(&mut self) {
				assert!(Self::ALLOW_SIGNED, "tried to negate an unsigned integer");
                *self = !(*self - 1);
			}
            #[inline(always)] fn qualified_max_length() -> usize { $qualified_max_length }
		})*
	}
}

#[cfg(target_pointer_width = "64")]
const SZ_USIZE: usize = 20;
#[cfg(target_pointer_width = "32")]
const SZ_USIZE: usize = 10;
#[cfg(target_pointer_width = "64")]
const SZ_ISIZE: usize = 20;
#[cfg(target_pointer_width = "32")]
const SZ_ISIZE: usize = 11;

impl_number_def! {
    usize {ALLOW_SIGNED, SZ_USIZE},
    // 255
    u8 {ALLOW_UNSIGNED, 3},
    // 65536
    u16 {ALLOW_UNSIGNED, 5},
    // 4294967296
    u32 {ALLOW_UNSIGNED, 10},
    // 18446744073709551616
    u64 {ALLOW_UNSIGNED, 20},
    // signed
    isize {ALLOW_SIGNED, SZ_ISIZE},
    // -128
    i8 {ALLOW_SIGNED, 4},
    // -32768
    i16 {ALLOW_SIGNED, 6},
    // -2147483648
    i32 {ALLOW_SIGNED, 11},
    // -9223372036854775808
    i64 {ALLOW_SIGNED, 20},
}

#[inline(always)]
fn decode_num_from_unbounded_payload<N>(src: &[u8], flag: &mut bool, cnt: &mut usize) -> N
where
    N: NumberDefinition,
{
    let l = src.len();
    let mut okay = !src.is_empty();
    let mut i = 0;
    let mut number = N::zero();
    let mut nx_stop = false;

    let is_signed = if N::ALLOW_SIGNED {
        let is_signed = i < l && src[i] == b'-';
        i += is_signed as usize;
        okay &= (i + 2) <= l; // [-][digit][LF]
        is_signed
    } else {
        false
    };

    while i < l && okay && !nx_stop {
        // potential exit
        nx_stop = src[i] == b'\n';
        // potential entry
        let mut local_ok = src[i].is_ascii_digit();
        let (p, p_of) = number.overflowing_mul(10);
        local_ok &= !p_of;
        let (s, s_of) = p.overflowing_add(src[i] & 0x0f);
        local_ok &= !s_of;
        // reassign or assign
        let reassign = number.b(nx_stop);
        let assign = s.b(!nx_stop);
        number = reassign | assign;
        okay &= local_ok | nx_stop;
        i += okay as usize;
    }
    okay &= nx_stop;
    *cnt += i;
    *flag &= okay;

    if N::ALLOW_SIGNED && is_signed {
        number.negate()
    }
    number
}

#[inline(always)]
fn decode_num_from_bounded_payload<N>(src: &[u8], flag: &mut bool) -> N
where
    N: NumberDefinition,
{
    let l = src.len();

    let mut i = 0;
    let mut number = N::zero();
    let mut okay = l <= N::qualified_max_length();

    if N::ALLOW_SIGNED {
        let is_signed = i < l && src[i] == b'-';
        if is_signed {
            number.negate();
        }
        i += is_signed as usize;
    }

    while i < l && okay {
        okay &= src[i].is_ascii_digit();
        let (product, p_of) = number.overflowing_mul(10);
        okay &= !p_of;
        let (sum, s_of) = product.overflowing_add(src[i] & 0x0F);
        okay &= !s_of;
        number = sum;
        i += 1;
    }

    *flag &= okay;
    number
}

#[derive(PartialEq, Debug, Clone, Copy)]
/// Intermediate literal repr
pub enum LitIR<'a> {
    Str(&'a str),
    Bin(&'a [u8]),
    UInt(u64),
    SInt(i64),
    Bool(bool),
    Float(f64),
}

#[derive(Debug, PartialEq)]
/// Data constructed from `opmode-safe`
pub struct SafeQueryData<'a> {
    p: Box<[LitIR<'a>]>,
    t: Vec<Token>,
}

impl<'a> SafeQueryData<'a> {
    #[cfg(test)]
    pub fn new_test(p: Box<[LitIR<'a>]>, t: Vec<Token>) -> Self {
        Self { p, t }
    }
    #[inline(always)]
    pub fn parse(qf: &'a [u8], pf: &'a [u8], pf_sz: usize) -> LangResult<Self> {
        let q = SafeLexer::lex(qf);
        let p = Self::p_revloop(pf, pf_sz);
        let (Ok(t), Ok(p)) = (q, p) else {
            return Err(LangError::UnexpectedChar)
        };
        Ok(Self { p, t })
    }
    #[inline]
    pub(super) fn p_revloop(mut src: &'a [u8], size: usize) -> LangResult<Box<[LitIR<'a>]>> {
        static LITIR_TF: [for<'a> fn(&'a [u8], &mut usize, &mut Vec<LitIR<'a>>) -> bool; 7] = [
            SafeQueryData::uint,  // tc: 0
            SafeQueryData::sint,  // tc: 1
            SafeQueryData::bool,  // tc: 2
            SafeQueryData::float, // tc: 3
            SafeQueryData::bin,   // tc: 4
            SafeQueryData::str,   // tc: 5
            |_, _, _| false,      // ecc: 6
        ];
        let nonpadded_offset = (LITIR_TF.len() - 2) as u8;
        let ecc_offset = LITIR_TF.len() - 1;
        let mut okay = true;
        let mut data = Vec::with_capacity(size);
        while src.len() >= 3 && okay {
            let tc = src[0];
            okay &= tc <= nonpadded_offset;
            let mx = cmp::min(ecc_offset, tc as usize);
            let mut i_ = 1;
            okay &= LITIR_TF[mx](&src[1..], &mut i_, &mut data);
            src = &src[i_..];
        }
        okay &= src.is_empty() && data.len() == size;
        if compiler::likely(okay) {
            Ok(data.into_boxed_slice())
        } else {
            Err(LangError::BadPframe)
        }
    }
}

// low level methods
impl<'b> SafeQueryData<'b> {
    #[inline(always)]
    fn mxple<'a>(src: &'a [u8], cnt: &mut usize, flag: &mut bool) -> &'a [u8] {
        // find payload length
        let mut i = 0;
        let payload_len = decode_num_from_unbounded_payload::<usize>(src, flag, &mut i);
        let src = &src[i..];
        // find payload
        *flag &= src.len() >= payload_len;
        let mx_extract = cmp::min(payload_len, src.len());
        // incr cursor
        i += mx_extract;
        *cnt += i;
        unsafe { slice::from_raw_parts(src.as_ptr(), mx_extract) }
    }
    #[inline(always)]
    pub(super) fn uint<'a>(src: &'a [u8], cnt: &mut usize, data: &mut Vec<LitIR<'a>>) -> bool {
        let mut b = true;
        let r = decode_num_from_unbounded_payload(src, &mut b, cnt);
        data.push(LitIR::UInt(r));
        b
    }
    #[inline(always)]
    pub(super) fn sint<'a>(src: &'a [u8], cnt: &mut usize, data: &mut Vec<LitIR<'a>>) -> bool {
        let mut b = true;
        let r = decode_num_from_unbounded_payload(src, &mut b, cnt);
        data.push(LitIR::SInt(r));
        b
    }
    #[inline(always)]
    pub(super) fn bool<'a>(src: &'a [u8], cnt: &mut usize, data: &mut Vec<LitIR<'a>>) -> bool {
        // `true\n` or `false\n`
        let mx = cmp::min(6, src.len());
        let slice = &src[..mx];
        let v_true = slice.starts_with(b"true\n");
        let v_false = slice.starts_with(b"false\n");
        let incr = v_true as usize * 5 + v_false as usize * 6;
        data.push(LitIR::Bool(v_true));
        *cnt += incr;
        v_true | v_false
    }
    #[inline(always)]
    pub(super) fn float<'a>(src: &'a [u8], cnt: &mut usize, data: &mut Vec<LitIR<'a>>) -> bool {
        let mut okay = true;
        let payload = Self::mxple(src, cnt, &mut okay);
        match String::from_utf8_lossy(payload).parse() {
            Ok(p) if okay => {
                data.push(LitIR::Float(p));
            }
            _ => {}
        }
        okay
    }
    #[inline(always)]
    pub(super) fn bin<'a>(src: &'a [u8], cnt: &mut usize, data: &mut Vec<LitIR<'a>>) -> bool {
        let mut okay = true;
        let payload = Self::mxple(src, cnt, &mut okay);
        data.push(LitIR::Bin(payload));
        okay
    }
    #[inline(always)]
    pub(super) fn str<'a>(src: &'a [u8], cnt: &mut usize, data: &mut Vec<LitIR<'a>>) -> bool {
        let mut okay = true;
        let payload = Self::mxple(src, cnt, &mut okay);
        match str::from_utf8(payload) {
            Ok(s) if okay => {
                data.push(LitIR::Str(s));
                true
            }
            _ => false,
        }
    }
}
