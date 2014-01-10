/// A TOML [1] configuration file parser
///
/// Copyright (c) 2014 by Michael Neumann
///
/// [1]: https://github.com/mojombo/toml

use std::io::Buffer;
use std::hashmap::HashMap;
use std::char;

use std::io::mem::MemReader;
use std::io::File;
use std::io::buffered::BufferedReader;
use std::path::Path;

#[deriving(ToStr,Clone)]
pub enum Value {
    NoValue,
    Boolean(bool),
    Unsigned(u64),
    Signed(u64),
    Float(f64),
    String(~str),
    Datetime(u16,u8,u8,u8,u8,u8),
    Array(~[Value]),
    TableArray(~[Value]),
    Table(HashMap<~str, Value>)
}

//
// This function determines if v1 and v2 have compatible ("equivalent") types
// as TOML allows only arrays where all elements are of the same type.
//
fn have_equiv_types(v1: &Value, v2: &Value) -> bool {
    match (v1, v2) {
        (&Boolean(_), &Boolean(_)) => true,
        (&Unsigned(_), &Unsigned(_)) => true,
        (&Unsigned(_), &Signed(_)) => true,
        (&Signed(_), &Unsigned(_)) => true,
        (&Signed(_), &Signed(_)) => true,
        (&Float(_), &Float(_)) => true,
        (&String(_), &String(_)) => true,
        (&Datetime(..), &Datetime(..)) => true,
        (&Array(_), &Array(_)) => true, // Arrays can be heterogenous in TOML
        _ => false
    }
}

enum PathElement<'a> {
    Key(&'a str),
    Idx(uint)
}

impl Value {
    pub fn get_bool(&self) -> Option<bool> {
        match self {
            &Boolean(b) => { Some(b) }
            _ => { None }
        }
    }

    pub fn get_int(&self) -> Option<i64> {
        match self {
            &Unsigned(u) => { Some(u as i64) } // XXX
            &Signed(u) => { Some(-(u as i64)) } // XXX
            _ => { None }
        }
    }

    pub fn get_float(&self) -> Option<f64> {
        match self {
            &Float(num) => { Some(num) }
            _ => { None } 
        }
    }

    pub fn get_str<'a>(&'a self) -> Option<&'a ~str> {
        match self {
            &String(ref str) => { Some(str) }
            _ => { None } 
        }
    }

    pub fn get_vec<'a>(&'a self) -> Option<&'a ~[Value]> {
        match self {
            &Array(ref vec) => { Some(vec) }
            _ => { None } 
        }
    }

    pub fn get_table<'a>(&'a self) -> Option<&'a HashMap<~str, Value>> {
        match self {
            &Table(ref table) => { Some(table) }
            _ => { None } 
        }
    }

    pub fn get_table_array<'a>(&'a self) -> Option<&'a ~[Value]> {
        match self {
            &TableArray(ref vec) => { Some(vec) }
            _ => { None } 
        }
    }

    pub fn lookup_key<'a>(&'a self, key: &str) -> Option<&'a Value> {
        match self {
            &Table(ref map) => {
                map.find_equiv(&key)
            }
            _ => { None }
        }
    }

    pub fn lookup_vec<'a>(&'a self, idx: uint) -> Option<&'a Value> {
        match self {
            &Array(ref ary) => {
                ary.get_opt(idx)
            }
            _ => { None }
        }
    }

    pub fn lookup_idx<'a>(&'a self, idx: uint) -> Option<&'a Value> {
        match self {
            &TableArray(ref tableary) => {
                tableary.get_opt(idx)
            }
            _ => { None }
        }
    }

    pub fn lookup_path<'a>(&'a self, path: &[&str]) -> Option<&'a Value> {
        if path.is_empty() {
            Some(self)
        } else {
            self.lookup_key(path[0]).and_then(|a| a.lookup_path(path.tail()))
        }
    }

    pub fn lookup_path_elts<'a>(&'a self, path: &[PathElement]) -> Option<&'a Value> {
        if path.is_empty() {
            Some(self)
        } else {
            match path[0] {
                Key(key) => self.lookup_key(key).and_then(|a| a.lookup_path_elts(path.tail())),
                Idx(idx) => self.lookup_idx(idx).and_then(|a| a.lookup_path_elts(path.tail()))
            }
        }
    }

    pub fn lookup<'a, 'b>(&'a self, path: &'b str) -> Option<&'a Value> {
        let paths: ~[&'b str] = path.split_str(".").collect();
        let path_elts: ~[PathElement<'b>] = paths.map(|&t| {
            let idx: Option<uint> = FromStr::from_str(t);
            if idx.is_some() {
                Idx(idx.unwrap())
            } else {
                Key(t)
            }
        });
        return self.lookup_path_elts(path_elts);
    }
}

trait Visitor {
    fn section(&mut self, name: ~str, is_array: bool) -> bool;
    fn pair(&mut self, key: ~str, val: Value) -> bool;
}

struct ValueBuilder {
    root: HashMap<~str, Value>,
    current_path: ~[~str]
}

impl ValueBuilder {
    fn new() -> ValueBuilder {
        ValueBuilder { root: HashMap::new(), current_path: ~[] }
    }

    fn recursive_create_tree(path: &[~str], ht: &mut HashMap<~str, Value>, is_array: bool) -> bool {
        assert!(path.len() > 0);

        if path.head().is_empty() { return false } // don"t allow empty keys 

        let term_rec: bool = (path.len() == 1);

        let head = path.head(); // TODO: optimize

        match ht.find_mut(head) {
            Some(&TableArray(ref mut table_array)) => {
                assert!(table_array.len() > 0);

                if term_rec { // terminal recursion
                    if is_array {
                        table_array.push(Table(HashMap::new()));
                        return true;
                    }
                    else {
                        debug!("Duplicate key");
                        return false;
                    }
                }
                else {
                    let mut last_table = &mut table_array[table_array.len()-1];
                    match last_table {
                        &Table(ref mut hmap) => {
                            return ValueBuilder::recursive_create_tree(path.tail(), hmap, is_array);
                        }
                        _ => {
                            // TableArray's only contain Table's
                            assert!(false);
                        }
                    }
                }
            }
            Some(&Table(ref mut table)) => {
                if term_rec { // terminal recursion
                    if is_array {
                        debug!("Duplicate key");
                        return false;
                    }
                    else {
                        return true;
                    }
                }
                else {
                    return ValueBuilder::recursive_create_tree(path.tail(), table, is_array);
                }
            }
            Some(_) => {
                debug!("Wrong type/duplicate key");
                return false;
            }
            None => {
                // fall-through, as we cannot modify 'ht' here
            }
        }

        let value =
        if term_rec { // terminal recursion
            if is_array { TableArray(~[Table(HashMap::new())]) }
            else { Table(HashMap::new()) }
        }
        else {
            let mut table = HashMap::new();
            let ok = ValueBuilder::recursive_create_tree(path.tail(), &mut table, is_array);
            if !ok { return false }
            Table(table)
        };
        let ok = ht.insert(head.to_owned(), value);
        assert!(ok);
        return ok;
    }

    fn insert_value(path: &[~str], key: &str, ht: &mut HashMap<~str, Value>, val: Value) -> bool {
        if path.is_empty() {
            return ht.insert(key.to_owned(), val);
        }
        else {
            let head = path.head(); // TODO: optimize
            match ht.find_mut(head) {
                Some(&Table(ref mut table)) => {
                    return ValueBuilder::insert_value(path.tail(), key, table, val);
                }
                Some(&TableArray(ref mut table_array)) => {
                    assert!(table_array.len() > 0);
                    let mut last_table = &mut table_array[table_array.len()-1];
                    match last_table {
                        &Table(ref mut hmap) => {
                            return ValueBuilder::insert_value(path.tail(), key, hmap, val);
                        }
                        _ => {
                            // TableArray's only contain Table's
                            assert!(false);
                            return false;
                        }
                    }
                }
                _ => {
                    debug!("Wrong type/duplicate key");
                    return false;
                }
            }
        }
    }

    fn get_root<'a>(&'a self) -> &'a HashMap<~str, Value> {
        return &self.root;
    }
}

impl Visitor for ValueBuilder {
    fn section(&mut self, name: ~str, is_array: bool) -> bool {
        self.current_path = name.split_str(".").map(|i| i.to_owned()).collect();

        let ok = ValueBuilder::recursive_create_tree(self.current_path.as_slice(), &mut self.root, is_array);
        if !ok {
            debug!("Duplicate section: {}", name);
        }
        return ok;
    }

    fn pair(&mut self, key: ~str, val: Value) -> bool {
        let ok = ValueBuilder::insert_value(self.current_path.as_slice(), key, &mut self.root, val);
        if !ok {
            debug!("Duplicate key: {} in path {:?}", key, self.current_path);
        }
        return ok;
    }
}

struct Parser<'a, BUF> {
    rd: &'a mut BUF,
    current_char: Option<char>,
    line: uint
}

impl<'a, BUF: Buffer> Parser<'a, BUF> {
    fn new(rd: &'a mut BUF) -> Parser<'a, BUF> {
        let ch = rd.read_char();
        let mut line = 1;
        if ch == Some('\n') { line += 1 }
        Parser { rd: rd, current_char: ch, line: line }
    }

    fn advance(&mut self) {
        self.current_char = self.rd.read_char();
    }

    fn get_line(&self) -> uint { self.line }

    fn ch(&self) -> Option<char> {
        return self.current_char;
    }

    fn eos(&self) -> bool {
        return self.current_char.is_none();
    }

    fn advance_if(&mut self, c: char) -> bool {
        match self.ch() {
            Some(ch) if ch == c => {
               self.advance();
               true
            }
            _ => {
                false
            }
        } 
    }

    fn read_digit(&mut self, radix: uint) -> Option<u8> {
        if self.eos() { return None }
        match char::to_digit(self.ch().unwrap(), radix) {
            Some(n) => {
                self.advance();
                Some(n as u8)
            }
            None => { None }
        }
    }

    fn read_two_digits(&mut self) -> Option<u8> {
        let d1 = self.read_digit(10);
        let d2 = self.read_digit(10);
        match (d1, d2) {
            (Some(d1), Some(d2)) => Some(d1*10+d2),
            _ => None
        }
    }

    fn read_digits(&mut self) -> (Option<u64>, uint) {
        let mut num: u64;
        match self.read_digit(10) {
            Some(n) => { num = n as u64; }
            None => { return (None, 0) }
        }
        let mut ndigits = 1;
        loop {
            match self.read_digit(10) {
                Some(n) => {
                    // XXX: check range
                    num = num * 10 + (n as u64);
                    ndigits += 1;
                }
                None => {
                    return (Some(num), ndigits)
                }
            }
        }
    }

    // allows a single "."
    fn read_float_mantissa(&mut self) -> f64 {
        let mut num: f64 = 0.0;
        let mut div: f64 = 10.0;

        loop {
            match self.read_digit(10) {
                Some(n) => {
                    num = num + (n as f64)/div;
                    div = div * 10.0;
                }
                None => {
                    return num;
                }
            }
        }
    }

    fn parse_float_rest(&mut self, n: u64, mul: f64) -> Value {
        if self.ch().is_none() { return NoValue }
        match self.ch().unwrap() {
            '0' .. '9' => {
                let num = self.read_float_mantissa();
                let num = (n as f64) + num;
                Float(num * mul)
            }
            _ => NoValue
        }
    }

    fn parse_value(&mut self) -> Value {
        self.skip_whitespaces_and_comments();

        if self.eos() { return NoValue }
        match self.ch().unwrap() {
            '-' => {
                self.advance();
                match self.read_digits() {
                    (Some(n), _) => {
                        if self.ch() == Some('.') {
                            // floating point
                            self.advance();
                            return self.parse_float_rest(n, -1.0);
                        }
                        else {
                            return Signed(n);
                        }
                    }
                    (None, _) => {
                        return NoValue
                    }
                }
            }
            '0' .. '9' => {
                match self.read_digits() {
                    (Some(n), ndigits) => {
                        match self.ch() {
                            Some('.') => {
                                // floating point
                                self.advance();
                                return self.parse_float_rest(n, 1.0);
                            }
                            Some('-') => {
                                if ndigits != 4 {
                                    debug!("Invalid Datetime");
                                    return NoValue;
                                }
                                self.advance();

                                let year = n;

                                let month = self.read_two_digits();
                                if month.is_none() || !self.advance_if('-') {
                                    debug!("Invalid Datetime");
                                    return NoValue;
                                }

                                let day = self.read_two_digits();
                                if day.is_none() || !self.advance_if('T'){
                                    debug!("Invalid Datetime");
                                    return NoValue;
                                }

                                let hour = self.read_two_digits();
                                if hour.is_none() || !self.advance_if(':') {
                                    debug!("Invalid Datetime");
                                    return NoValue;
                                }

                                let min = self.read_two_digits();
                                if min.is_none() || !self.advance_if(':') {
                                    debug!("Invalid Datetime");
                                    return NoValue;
                                }

                                let sec = self.read_two_digits();
                                if sec.is_none() || !self.advance_if('Z') {
                                    debug!("Invalid Datetime");
                                    return NoValue;
                                }

                                match (year, month, day, hour, min, sec) {
                                    (y, Some(m), Some(d),
                                     Some(h), Some(min), Some(s))
                                    if m > 0 && m <= 12 && d > 0 && d <= 31 &&
                                       h <= 24 && min <= 60 && s <= 60 => {
                                        return Datetime(y as u16,m,d,h,min,s)
                                    }
                                    _ => {
                                        debug!("Invalid Datetime range");
                                        return NoValue;
                                    }
                                }
                            }
                            _ => {
                                return Unsigned(n)
                            }
                        }
                    }
                    (None, _) => {
                        assert!(false);
                        return NoValue
                    }
                }
            }
            't' => {
                self.advance();
                if self.advance_if('r') &&
                   self.advance_if('u') &&
                   self.advance_if('e') {
                    return Boolean(true)
                } else {
                    return NoValue
                }
            
        }
            'f' => {
                self.advance();
                if self.advance_if('a') &&
                   self.advance_if('l') &&
                   self.advance_if('s') && 
                   self.advance_if('e') {
                    return Boolean(false)
                } else {
                    return NoValue
                }
            }
            '[' => {
                self.advance();
                let mut arr = ~[];
                loop {
                    match self.parse_value() {
                        NoValue => {
                            break;
                        }
                        val => {
                            if !arr.is_empty() {
                                if !have_equiv_types(arr.head(), &val) {
                                    debug!("Incompatible element types in array");
                                    return NoValue;
                                }
                            }
                            arr.push(val);
                        }
                    }
                    
                    self.skip_whitespaces_and_comments();
                    if !self.advance_if(',') { break }
                }
                self.skip_whitespaces_and_comments();
                if self.advance_if(']') {
                    return Array(arr);
                } else {
                    return NoValue;
                }
            }
            '"' => {
                match self.parse_string() {
                    Some(str) => { return String(str) }
                    None => { return NoValue }
                }
            }
            _ => { return NoValue }
        }
    }

    fn parse_string(&mut self) -> Option<~str> {
        if !self.advance_if('"') { return None }

        let mut str = ~"";
        loop {
            if self.ch().is_none() { return None }
            match self.ch().unwrap() {
                '\r' | '\n' | '\u000C' | '\u0008' => { return None }
                '\\' => {
                    self.advance();
                    if self.ch().is_none() { return None }
                    match self.ch().unwrap() {
                        'b' => { str.push_char('\u0008'); self.advance() },
                        't' => { str.push_char('\t'); self.advance() },
                        'n' => { str.push_char('\n'); self.advance() },
                        'f' => { str.push_char('\u000C'); self.advance() },
                        'r' => { str.push_char('\r'); self.advance() },
                        '"' => { str.push_char('"'); self.advance() },
                        '/' => { str.push_char('/'); self.advance() },
                        '\\' => { str.push_char('\\'); self.advance() },
                        'u' => {
                            self.advance();
                            let d1 = self.read_digit(16);
                            let d2 = self.read_digit(16);
                            let d3 = self.read_digit(16);
                            let d4 = self.read_digit(16);
                            match (d1, d2, d3, d4) {
                                (Some(d1), Some(d2), Some(d3), Some(d4)) => {
                                    // XXX: how to construct an UTF character
                                    let ch = (((((d1 as u32 << 4) | d2 as u32) << 4) | d3 as u32) << 4) | d4 as u32;
                                    match char::from_u32(ch) {
                                        Some(ch) => {
                                            str.push_char(ch);
                                        }
                                        None => {
                                            return None;
                                        }
                                    }
                                }
                                _ => return None
                            }
                        }
                        _ => { return None }
                    }
                }
                '"' => {
                    self.advance();
                    return Some(str);
                }
                c => {
                    str.push_char(c);
                    self.advance();
                }
            }
        }
    }

    fn read_token(&mut self, f: |char| -> bool) -> ~str {
        let mut token = ~"";
        loop {
            match self.ch() {
                Some(ch) => {
                    if f(ch) { token.push_char(ch) }
                    else { break }
                }
                None => { break }
            }
            self.advance();
        }

        return token;
    }

    fn parse_section_identifier(&mut self) -> ~str {
        self.read_token(|ch| {
            match ch {
                '\t' | '\n' | '\r' | '[' | ']' => false,
                _ => true
            }
        })
    }

    fn skip_whitespaces(&mut self) {
        loop {
            match self.ch() {
                Some(' ') | Some('\t') | Some('\r') => {
                    self.advance();
                }
                Some('\n') => {
                    self.advance();
                    self.line += 1;
                }
                _ => { break }
            }
        }
    }

    fn skip_whitespaces_and_comments(&mut self) {
        loop {
            match self.ch() {
                Some(' ') | Some('\t') | Some('\r') => {
                    self.advance();
                }
                Some('\n') => {
                    self.advance();
                    self.line += 1;
                }
                Some('#') => {
                    self.skip_comment();
                }
                _ => { break }
            }
        }
    }

    fn skip_comment(&mut self) {
        assert!(self.ch() == Some('#'));
        // skip to end of line
        loop {
            self.advance();
            match self.ch() {
                Some('\n') => { break }
                None => { return }
                _ => { /* skip */ }
            }
        }
        self.line += 1;
        self.advance();
    }

    fn parse<V: Visitor>(&mut self, visitor: &mut V) -> bool {
        loop {
            self.skip_whitespaces_and_comments();

            if self.eos() { return true }

            match self.ch().unwrap() {
                // section
                '[' => {
                    self.advance();
                    let mut double_section = false;
                    match self.ch() {
                        Some('[') => {
                            double_section = true;
                            self.advance();
                        }
                        _ => {}
                    }

                    let section_name = self.parse_section_identifier();
                    // don"t allow empty section names
                    if section_name.is_empty() { return false }

                    if !self.advance_if(']') { return false }
                    if double_section {
                        if !self.advance_if(']') { return false }
                    }

                    if !visitor.section(section_name, double_section) {
                        return false
                    }
                }

                // identifier: anything else starts an idenfifier!
                // NOTE that we do not allow '.' in identifiers!
                _ => {
                    let ident = self.read_token(|ch| {
                        match ch {
                            ' ' | '\t' | '\r' | '\n' | '=' => false,
                            _ => true
                        }
                    });

                    self.skip_whitespaces();

                    if !self.advance_if('=') { return false } // assign wanted
                    
                    match self.parse_value() {
                        NoValue => { return false; }
                        val => { 
                            if !visitor.pair(ident, val) { return false; }
                        }
                    }
                }
            } /* end match */
        }

        assert!(false);
    }
}

pub fn parse_from_file(name: &str) -> Value {
    let path = Path::new(name);
    let mut file = File::open(&path);

    let mut rd = BufferedReader::new(file);
    return parse_from_buffer(&mut rd);
}

pub fn parse_from_buffer<BUF: Buffer>(rd: &mut BUF) -> Value {
    let mut builder = ValueBuilder::new();
    let mut parser = Parser::new(rd);
    if parser.parse(&mut builder) {
        return Table(builder.get_root().clone());
    } else {
        debug!("Error in line: {}", parser.get_line());
        return NoValue;
    }
}

pub fn parse_from_bytes(bytes: ~[u8]) -> Value {
    let mut rd = MemReader::new(bytes);
    return parse_from_buffer(&mut rd);
}
