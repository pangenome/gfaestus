pub mod gui;
pub mod mainview;

use crossbeam::channel;

use handlegraph::handle::NodeId;

use crate::geometry::*;
use crate::input::MousePos;
use crate::view::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppMsg {
    SelectNode(Option<NodeId>),
    HoverNode(Option<NodeId>),
}

pub struct App {
    mouse_pos: MousePos,
    screen_dims: ScreenDims,

    hover_node: Option<NodeId>,
    selected_node: Option<NodeId>,
}

impl App {
    pub fn new<Dims: Into<ScreenDims>>(
        mouse_pos: MousePos,
        screen_dims: Dims,
    ) -> Self {
        Self {
            mouse_pos,
            screen_dims: screen_dims.into(),
            hover_node: None,
            selected_node: None,
        }
    }

    pub fn hover_node(&self) -> Option<NodeId> {
        self.hover_node
    }

    pub fn selected_node(&self) -> Option<NodeId> {
        self.selected_node
    }

    pub fn dims(&self) -> ScreenDims {
        self.screen_dims
    }

    pub fn mouse_pos(&self) -> Point {
        self.mouse_pos.read()
    }

    pub fn update_dims<Dims: Into<ScreenDims>>(&mut self, screen_dims: Dims) {
        self.screen_dims = screen_dims.into();
    }

    pub fn apply_app_msg(&mut self, msg: &AppMsg) {
        match msg {
            AppMsg::SelectNode(id) => self.selected_node = *id,
            AppMsg::HoverNode(id) => self.hover_node = *id,
        }
    }
}
