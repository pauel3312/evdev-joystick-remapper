pub struct IdPool<T> {
    data: Vec<Option<T>>,
    free_ids: Vec<i16>,
}

impl<T> IdPool<T> {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            free_ids: Vec::new(),
        }
    }

    pub fn insert(&mut self, value: T) -> i16 {
        if let Some(id) = self.free_ids.pop() {
            self.data[id as usize] = Some(value);
            id
        } else {
            let id = self.data.len() as i16;
            self.data.push(Some(value));
            id
        }
    }

    pub fn remove(&mut self, id: i16) -> Option<T> {
        let slot = self.data.get_mut(id as usize)?;
        let value = slot.take();
        if value.is_some() {
            self.free_ids.push(id);
        }
        value
    }

    pub fn get(&self, id: i16) -> Option<&T> {
        self.data.get(id as usize)?.as_ref()
    }
}
