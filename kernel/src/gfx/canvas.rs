pub trait Canvas {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn put_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8);
    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, r: u8, g: u8, b: u8);
    fn scroll(&mut self, lines_pixels: usize, bg_r: u8, bg_g: u8, bg_b: u8);
}
