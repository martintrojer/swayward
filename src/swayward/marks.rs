use super::*;

impl Swayward {
    pub fn set_mark(&mut self, window: MappedId, mark: &str, add: bool, toggle: bool) {
        let had_mark = self.marks.get(mark) == Some(&window);
        if !add {
            if let Some(existing) = self.marks_by_window.remove(&window) {
                for mark in existing {
                    self.marks.remove(&mark);
                }
            }
        }
        if let Some(previous) = self.marks.remove(mark) {
            if let Some(marks) = self.marks_by_window.get_mut(&previous) {
                marks.retain(|existing| existing != mark);
            }
        }
        if !toggle || !had_mark {
            self.marks.insert(mark.to_owned(), window);
            self.marks_by_window
                .entry(window)
                .or_default()
                .push(mark.to_owned());
        }
    }

    pub fn remap_container_marks(
        &mut self,
        remapped: impl IntoIterator<
            Item = (
                crate::layout::tiling_tree::NodeId,
                crate::layout::tiling_tree::NodeId,
            ),
        >,
    ) {
        let moved = remapped
            .into_iter()
            .filter_map(|(old, new)| Some((new, self.marks_by_container.remove(&old)?)))
            .collect::<Vec<_>>();
        for (new, marks) in moved {
            self.marks_by_container
                .entry(new)
                .or_default()
                .extend(marks);
        }
    }

    pub fn unmark(&mut self, window: Option<MappedId>, mark: Option<&str>) {
        match (window, mark) {
            (Some(window), Some(mark)) if self.marks.get(mark) == Some(&window) => {
                self.marks.remove(mark);
                if let Some(marks) = self.marks_by_window.get_mut(&window) {
                    marks.retain(|existing| existing != mark);
                }
            }
            (Some(window), None) => {
                for mark in self.marks_by_window.remove(&window).unwrap_or_default() {
                    self.marks.remove(&mark);
                }
            }
            (None, Some(mark)) => {
                if let Some(window) = self.marks.remove(mark) {
                    if let Some(marks) = self.marks_by_window.get_mut(&window) {
                        marks.retain(|existing| existing != mark);
                    }
                }
            }
            (None, None) => {
                self.marks.clear();
                self.marks_by_window.clear();
            }
            _ => {}
        }
    }
}
