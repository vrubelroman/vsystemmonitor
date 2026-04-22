pub struct Pager {
    current_page: usize,
    page_size: usize,
}

impl Pager {
    pub fn new(page_size: usize) -> Self {
        Self {
            current_page: 0,
            page_size: page_size.max(1),
        }
    }

    pub fn current_page(&self) -> usize {
        self.current_page
    }

    pub fn set_page_size(&mut self, page_size: usize) {
        self.page_size = page_size.max(1);
    }

    pub fn next_page(&mut self, total_items: usize) {
        let total_pages = self.total_pages(total_items);
        if total_pages > 0 {
            self.current_page = (self.current_page + 1) % total_pages;
        }
    }

    pub fn prev_page(&mut self, total_items: usize) {
        let total_pages = self.total_pages(total_items);
        if total_pages > 0 {
            self.current_page = if self.current_page == 0 {
                total_pages - 1
            } else {
                self.current_page - 1
            };
        }
    }

    pub fn clamp(&mut self, total_items: usize) {
        let total_pages = self.total_pages(total_items);
        if total_pages == 0 {
            self.current_page = 0;
        } else if self.current_page >= total_pages {
            self.current_page = total_pages - 1;
        }
    }

    pub fn total_pages(&self, total_items: usize) -> usize {
        if total_items == 0 {
            0
        } else {
            total_items.div_ceil(self.page_size)
        }
    }

    pub fn window(&self, total_items: usize) -> (usize, usize) {
        if total_items == 0 {
            return (0, 0);
        }

        let start = self.current_page * self.page_size;
        let end = (start + self.page_size).min(total_items);
        (start, end)
    }
}
