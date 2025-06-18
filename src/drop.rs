pub fn on_drop(f: impl FnOnce()) -> impl Drop {
    struct Dropper<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for Dropper<F> {
        fn drop(&mut self) {
            (self.0.take().unwrap())();
        }
    }
    Dropper(Some(f))
}
