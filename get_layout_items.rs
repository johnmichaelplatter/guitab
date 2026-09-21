enum LayoutItem<'a> {
    Event(&'a crate::mnx::Event),
    TupletStart,
    TupletEnd,
}
