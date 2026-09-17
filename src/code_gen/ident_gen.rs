pub struct IdentGen {
    last: Vec<u8>
}

impl IdentGen {
    pub fn new() -> Self {
        Self { last: vec!['a' as u8 - 1] }
    }

    fn next(&mut self) -> u8 {
        let prev = self.last.last_mut().unwrap();
        if *prev as char == 'z' {
            self.last.push('a' as u8);
            return 'a' as u8;
        }

        *prev += 1;
        *prev
    }

    pub fn gen_ident(&mut self) -> String {
        self.next();
        String::from_utf8(self.last.clone()).unwrap()
    }
}