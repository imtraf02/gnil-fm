#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileManagerSurfaceKind {
    Sidebar,
    Header,
    FileList,
    Preview,
    Status,
}

impl FileManagerSurfaceKind {
    const ALL: [Self; 5] = [
        Self::Sidebar,
        Self::Header,
        Self::FileList,
        Self::Preview,
        Self::Status,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Sidebar => 0,
            Self::Header => 1,
            Self::FileList => 2,
            Self::Preview => 3,
            Self::Status => 4,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::Header => "header",
            Self::FileList => "list",
            Self::Preview => "preview",
            Self::Status => "status",
        }
    }
}

struct FileManagerSurface {
    owner: gpui::WeakEntity<FileManager>,
    kind: FileManagerSurfaceKind,
    #[cfg(test)]
    render_count: u64,
}

impl FileManagerSurface {
    fn new(owner: gpui::WeakEntity<FileManager>, kind: FileManagerSurfaceKind) -> Self {
        Self {
            owner,
            kind,
            #[cfg(test)]
            render_count: 0,
        }
    }
}

impl Render for FileManagerSurface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.render_count = self.render_count.saturating_add(1);
        }
        self.owner
            .update(cx, |owner, owner_cx| {
                let started = Instant::now();
                let element = match self.kind {
                    FileManagerSurfaceKind::Sidebar => owner.render_sidebar(owner_cx),
                    FileManagerSurfaceKind::Header => owner.render_header(owner_cx),
                    FileManagerSurfaceKind::FileList => {
                        let list_focused = owner.focus_handle.is_focused(window);
                        owner.render_file_list(list_focused, owner_cx)
                    }
                    FileManagerSurfaceKind::Preview => owner.render_preview(),
                    FileManagerSurfaceKind::Status => owner.render_status(owner_cx),
                };
                owner
                    .perf_trace
                    .record_surface(self.kind, started.elapsed());
                element
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

#[derive(Clone)]
struct FileManagerSurfaces {
    sidebar: Entity<FileManagerSurface>,
    header: Entity<FileManagerSurface>,
    file_list: Entity<FileManagerSurface>,
    preview: Entity<FileManagerSurface>,
    status: Entity<FileManagerSurface>,
}

impl FileManagerSurfaces {
    fn install(cx: &mut Context<FileManager>) -> (Self, Subscription) {
        let surfaces = Self::new(&cx.weak_entity(), cx);
        let surfaces_for_observer = surfaces.clone();
        let subscription = cx.observe_self(move |_, cx| {
            surfaces_for_observer.invalidate_all(cx);
        });
        (surfaces, subscription)
    }

    fn new(owner: &gpui::WeakEntity<FileManager>, cx: &mut Context<FileManager>) -> Self {
        let surface = |kind, cx: &mut Context<FileManager>| {
            let owner = owner.clone();
            cx.new(|_| FileManagerSurface::new(owner, kind))
        };
        Self {
            sidebar: surface(FileManagerSurfaceKind::Sidebar, cx),
            header: surface(FileManagerSurfaceKind::Header, cx),
            file_list: surface(FileManagerSurfaceKind::FileList, cx),
            preview: surface(FileManagerSurfaceKind::Preview, cx),
            status: surface(FileManagerSurfaceKind::Status, cx),
        }
    }

    fn invalidate_all(&self, cx: &mut Context<FileManager>) {
        for surface in [
            &self.sidebar,
            &self.header,
            &self.file_list,
            &self.preview,
            &self.status,
        ] {
            surface.update(cx, |_, cx| cx.notify());
        }
    }

    fn invalidate_file_list(&self, cx: &mut Context<FileManager>) {
        self.file_list.update(cx, |_, cx| cx.notify());
    }

    fn sidebar(&self) -> AnyView {
        AnyView::from(self.sidebar.clone()).cached(
            StyleRefinement::default()
                .w(px(218.0))
                .h_full()
                .flex_none(),
        )
    }

    fn header(&self) -> AnyView {
        AnyView::from(self.header.clone()).cached(
            StyleRefinement::default()
                .h(px(58.0))
                .w_full()
                .flex_none(),
        )
    }

    fn file_list(&self) -> AnyView {
        AnyView::from(self.file_list.clone()).cached(
            StyleRefinement::default()
                .w_full()
                .h_full()
                .min_w_0()
                .min_h_0(),
        )
    }

    fn preview(&self) -> AnyView {
        AnyView::from(self.preview.clone()).cached(
            StyleRefinement::default()
                .w(px(318.0))
                .min_w(px(240.0))
                .h_full()
                .flex_none(),
        )
    }

    fn status(&self) -> AnyView {
        AnyView::from(self.status.clone()).cached(
            StyleRefinement::default()
                .h(px(30.0))
                .w_full()
                .flex_none(),
        )
    }
}
