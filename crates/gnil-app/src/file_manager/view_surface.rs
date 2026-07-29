#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileManagerSurfaceKind {
    Sidebar,
    Header,
    CommandBar,
    FileList,
    RubberBand,
    Preview,
    Status,
}

impl FileManagerSurfaceKind {
    const ALL: [Self; 7] = [
        Self::Sidebar,
        Self::Header,
        Self::CommandBar,
        Self::FileList,
        Self::RubberBand,
        Self::Preview,
        Self::Status,
    ];
    const COUNT: usize = Self::ALL.len();

    const fn index(self) -> usize {
        match self {
            Self::Sidebar => 0,
            Self::Header => 1,
            Self::CommandBar => 2,
            Self::FileList => 3,
            Self::RubberBand => 4,
            Self::Preview => 5,
            Self::Status => 6,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::Header => "header",
            Self::CommandBar => "command_bar",
            Self::FileList => "list",
            Self::RubberBand => "rubber",
            Self::Preview => "preview",
            Self::Status => "status",
        }
    }
}

#[derive(Clone, Copy)]
struct SurfaceMask(u8);

impl SurfaceMask {
    const SIDEBAR: Self = Self(1 << 0);
    const HEADER: Self = Self(1 << 1);
    const COMMAND_BAR: Self = Self(1 << 2);
    const FILE_LIST: Self = Self(1 << 3);
    const RUBBER_BAND: Self = Self(1 << 4);
    const PREVIEW: Self = Self(1 << 5);
    const STATUS: Self = Self(1 << 6);
    const NAVIGATION: Self = Self(
        Self::SIDEBAR.0
            | Self::HEADER.0
            | Self::COMMAND_BAR.0
            | Self::FILE_LIST.0
            | Self::PREVIEW.0
            | Self::STATUS.0,
    );

    const fn contains(self, kind: FileManagerSurfaceKind) -> bool {
        self.0 & (1 << kind.index()) != 0
    }
}

impl std::ops::BitOr for SurfaceMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
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
                let started = owner.perf_trace.start();
                let element = match self.kind {
                    FileManagerSurfaceKind::Sidebar => owner.render_sidebar(owner_cx),
                    FileManagerSurfaceKind::Header => owner.render_header(window, owner_cx),
                    FileManagerSurfaceKind::CommandBar => {
                        owner.render_command_bar(window, owner_cx)
                    }
                    FileManagerSurfaceKind::FileList => {
                        let list_focused = owner.focus_handle.is_focused(window);
                        owner.render_file_list(list_focused, window, owner_cx)
                    }
                    FileManagerSurfaceKind::RubberBand => owner.render_rubber_band(),
                    FileManagerSurfaceKind::Preview => owner.render_preview(),
                    FileManagerSurfaceKind::Status => owner.render_status(owner_cx),
                };
                owner.perf_trace.record_surface(self.kind, started);
                element
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

#[derive(Clone)]
struct FileManagerSurfaces {
    sidebar: Entity<FileManagerSurface>,
    header: Entity<FileManagerSurface>,
    command_bar: Entity<FileManagerSurface>,
    file_list: Entity<FileManagerSurface>,
    rubber_band: Entity<FileManagerSurface>,
    preview: Entity<FileManagerSurface>,
    status: Entity<FileManagerSurface>,
}

impl FileManagerSurfaces {
    fn install(cx: &mut Context<FileManager>) -> Self {
        Self::new(&cx.weak_entity(), cx)
    }

    fn new(owner: &gpui::WeakEntity<FileManager>, cx: &mut Context<FileManager>) -> Self {
        let surface = |kind, cx: &mut Context<FileManager>| {
            let owner = owner.clone();
            cx.new(|_| FileManagerSurface::new(owner, kind))
        };
        Self {
            sidebar: surface(FileManagerSurfaceKind::Sidebar, cx),
            header: surface(FileManagerSurfaceKind::Header, cx),
            command_bar: surface(FileManagerSurfaceKind::CommandBar, cx),
            file_list: surface(FileManagerSurfaceKind::FileList, cx),
            rubber_band: surface(FileManagerSurfaceKind::RubberBand, cx),
            preview: surface(FileManagerSurfaceKind::Preview, cx),
            status: surface(FileManagerSurfaceKind::Status, cx),
        }
    }

    fn invalidate(&self, mask: SurfaceMask, cx: &mut Context<FileManager>) {
        for (kind, surface) in [
            (FileManagerSurfaceKind::Sidebar, &self.sidebar),
            (FileManagerSurfaceKind::Header, &self.header),
            (FileManagerSurfaceKind::CommandBar, &self.command_bar),
            (FileManagerSurfaceKind::FileList, &self.file_list),
            (FileManagerSurfaceKind::RubberBand, &self.rubber_band),
            (FileManagerSurfaceKind::Preview, &self.preview),
            (FileManagerSurfaceKind::Status, &self.status),
        ] {
            if mask.contains(kind) {
                surface.update(cx, |_, cx| cx.notify());
            }
        }
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

    fn command_bar(&self) -> AnyView {
        AnyView::from(self.command_bar.clone()).cached(
            StyleRefinement::default()
                .h(px(44.0))
                .w_full()
                .flex_none(),
        )
    }

    fn rubber_band(&self) -> AnyView {
        AnyView::from(self.rubber_band.clone()).cached(
            StyleRefinement::default()
                .absolute()
                .inset_0()
                .w_full()
                .h_full(),
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

impl FileManager {
    fn invalidate_surfaces(&mut self, mask: SurfaceMask, cx: &mut Context<Self>) {
        self.perf_trace.record_invalidation(mask);
        self.surfaces.invalidate(mask, cx);
    }
}
