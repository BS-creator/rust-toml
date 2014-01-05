/// A TOML [1] configuration file parser
///
/// [1]: https://github.com/mojombo/toml

use std::io::mem::MemReader;
use std::io::File;
use std::hashmap::HashMap;

#[deriving(ToStr)]
enum Value {
    True,
    False,
    Unsigned(u64),
    Integer(i64),
    Float(f64),
    String(~str),
    Array(~[Value]),
    Datetime, // XXX
    Map(HashMap<~str, Value>) // XXX: This is no value
}

trait Visitor {
    fn section(&mut self, name: ~str, is_array: bool) -> bool;
    fn pair(&mut self, key: ~str, val: Value) -> bool;
}

struct TOMLVisitor {
    root: HashMap<~str, Value>,
    current_section: ~str,
    section_is_array: bool
}

impl TOMLVisitor {
    fn new() -> TOMLVisitor {
        TOMLVisitor { root: HashMap::new(), current_section: ~"", section_is_array: false }
    }
    fn get_root<'a>(&'a self) -> &'a HashMap<~str, Value> {
        return &self.root;
    }
}

impl Visitor for TOMLVisitor {
    fn section(&mut self, name: ~str, is_array: bool) -> bool {
        debug!("Section: {} (is_array={})", name, is_array);
        self.section_is_array = is_array;
        self.current_section = name;
        return true
    }
    fn pair(&mut self, key: ~str, val: Value) -> bool {
        debug!("Pair: {} {:s}", key, val.to_str());
        let m = self.root.find_or_insert(self.current_section.clone(), Map(HashMap::new())); // XXX: remove clone
        match *m {
            Map(ref mut map) => {
                let ok = map.insert(key, val);
                return ok
            }
            _ => { return false }
        }
    }
}

struct Parser<'a> {
    rd: &'a mut MemReader,
    current_char: Option<char>
}

impl<'a> Parser<'a> {
    fn read_char(rd: &mut MemReader) -> Option<char> {
        match rd.read_byte() {
            Some(b) => Some(b as char),
            None => None
        }
    }

    fn new(rd: &'a mut MemReader) -> Parser<'a> {
        let ch = Parser::read_char(rd);
        Parser { rd: rd, current_char: ch }
    }

    fn advance(&mut self) {
        self.current_char = Parser::read_char(self.rd)
    }

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
        match std::char::to_digit(self.ch().unwrap(), radix) {
            Some(n) => {
                self.advance();
                Some(n as u8)
            }
            None => { None }
        }
    }

    fn read_digits(&mut self) -> Option<u64> {
        let mut num: u64;
        match self.read_digit(10) {
            Some(n) => { num = n as u64; }
            None => { return None }
        }
        loop {
            match self.read_digit(10) {
                Some(n) => {
                    // XXX: check range
                    num = num * 10 + (n as u64);
                }
                None => {
                    return Some(num)
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

    fn parse_value(&mut self) -> Option<Value> {
        self.skip_whitespaces();

        if self.eos() { return None }
        match self.ch().unwrap() {
            '-' => {
                self.advance();
                match self.read_digits() {
                    Some(n) => {
                        if self.ch() == Some('.') {
                            // floating point
                            self.advance();
                            let num = self.read_float_mantissa();
                            let num = (n as f64) + num;
                            return Some(Float(-num));
                        }
                        else {
                            match n.to_i64() {
                                Some(i) => Some(Integer(-i)),
                                None => None // XXX: Use Result
                            }
                        }
                    }
                    None => {
                        return None
                    }
                }
            }
            '0' .. '9' => {
                match self.read_digits() {
                    Some(n) => {
                        match self.ch() {
                            Some('.') => {
                                // floating point
                                self.advance();
                                let num = self.read_float_mantissa();
                                let num = (n as f64) + num;
                                return Some(Float(num));
                            }
                            Some('-') => {
                                // XXX
                                fail!("Datetime not yet supported");
                            }
                            _ => {
                                return Some(Unsigned(n))
                            }
                        }
                    }
                    None => {
                        assert!(false);
                        return None
                    }
                }
            }
            't' => {
                self.advance();
                if self.advance_if('r') &&
                   self.advance_if('u') &&
                   self.advance_if('e') {
                    return Some(True)
                } else {
                    return None
                }
            }
            'f' => {
                self.advance();
                if self.advance_if('a') &&
                   self.advance_if('l') &&
                   self.advance_if('s') && 
                   self.advance_if('e') {
                    return Some(True)
                } else {
                    return None
                }
            }
            '[' => {
                self.advance();
                let mut arr = ~[];
                loop {
                    match self.parse_value() {
                        Some(val) => {
                            arr.push(val);
                        }
                        None => {
                            break;
                        }
                    }
                    
                    self.skip_whitespaces_and_comments();
                    if !self.advance_if(',') { break }
                }
                self.skip_whitespaces_and_comments();
                if self.advance_if(']') {
                    return Some(Array(arr));
                } else {
                    return None;
                }
            }
            '"' => {
                match self.parse_string() {
                    Some(str) => { return Some(String(str)) }
                    None => { return None }
                }
            }
            _ => { return None }
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
                                    let ch = (((((d1 as u32 << 8) | d2 as u32) << 8) | d3 as u32) << 8) | d4 as u32;
                                    match std::char::from_u32(ch) {
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
                    let len = std::char::len_utf8_bytes(c);
                    //assert!(len >= 1 && len <= 4);
                    assert!(len == 1);
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
                'a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '.' | '_' => true,
                _ => false
            }
        })
    }

    fn skip_whitespaces(&mut self) {
        loop {
            match self.ch() {
                Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                    self.advance();
                }
                _ => { break }
            }
        }
    }

    fn skip_whitespaces_and_comments(&mut self) {
        loop {
            match self.ch() {
                Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                    self.advance();
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
        self.advance();
    }

    fn parse<V: Visitor>(&mut self, visitor: &mut V) -> bool {
        loop {
            if self.eos() { return true }
            match self.ch().unwrap() {
                // ignore whitespace
                '\r' | '\n' | ' ' | '\t' => {
                    self.advance();
                }

                // comment
                '#' => {
                    self.skip_comment();
                }

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

                    if !self.advance_if(']') { return false }
                    if double_section {
                        if !self.advance_if(']') { return false }
                    }

                    visitor.section(section_name, double_section);
                }

                // identifier
                'a' .. 'z' | 'A' .. 'Z' | '_' => {

                    let ident = self.read_token(|ch| {
                        match ch {
                            'a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '_' => true,
                            _ => false
                        }
                    });

                    self.skip_whitespaces();

                    if !self.advance_if('=') { return false } // assign wanted
                    
                    match self.parse_value() {
                        Some(val) => { visitor.pair(ident, val); }
                        None => { return false; }
                    }
                    // do not advance!
                }

                _ => { return false }
            } /* end match */
        }

        assert!(false);
    }
}

fn main() {
  let contents = File::open(&Path::new(std::os::args()[1])).read_to_end();
  let mut visitor = TOMLVisitor::new();
  let mut rd = MemReader::new(contents);
  let mut parser = Parser::new(&mut rd);
  parser.parse(&mut visitor);
  println!("{:s}", visitor.get_root().to_str());
}
