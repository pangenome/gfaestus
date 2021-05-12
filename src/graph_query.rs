use futures::{executor::ThreadPool, Future};
#[allow(unused_imports)]
use handlegraph::{
    handle::{Direction, Handle, NodeId},
    handlegraph::*,
    mutablehandlegraph::*,
    packed::*,
    pathhandlegraph::*,
};

use handlegraph::{
    packedgraph::{paths::StepPtr, PackedGraph},
    path_position::PathPositionMap,
};

use crossbeam::{
    atomic::AtomicCell,
    channel::{self, Receiver},
};

use std::sync::Arc;

use anyhow::Result;

use crate::asynchronous::AsyncResult;

pub struct GraphQueryWorker {
    graph_query: Arc<GraphQuery>,
    thread_pool: Arc<ThreadPool>,
}

impl GraphQueryWorker {
    pub fn new(
        graph_query: Arc<GraphQuery>,
        thread_pool: Arc<ThreadPool>,
    ) -> Self {
        Self {
            graph_query,
            thread_pool,
        }
    }

    pub fn run_query<T, F, Fut>(&self, query: F) -> AsyncResult<T>
    where
        T: Send,
        F: FnOnce(Arc<GraphQuery>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let future = query(self.graph_query.clone());

        let result = AsyncResult::new(&self.thread_pool, future);

        result
    }
}

pub struct GraphQuery {
    graph: Arc<PackedGraph>,
    path_positions: Arc<PathPositionMap>,
    query_thread: QueryThread,
}

impl GraphQuery {
    pub fn load_gfa(gfa_path: &str) -> Result<Self> {
        let mut mmap = gfa::mmap::MmapGFA::new(gfa_path)?;
        let graph = crate::gfa::load::packed_graph_from_mmap(&mut mmap)?;
        let path_positions = PathPositionMap::index_paths(&graph);
        Ok(Self::new(graph, path_positions))
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn new(graph: PackedGraph, path_positions: PathPositionMap) -> Self {
        let graph = Arc::new(graph);
        let path_positions = Arc::new(path_positions);
        let query_thread = QueryThread::new(graph.clone());
        Self {
            graph,
            path_positions,
            query_thread,
        }
    }

    pub fn query_request_blocking(
        &self,
        request: GraphQueryRequest,
    ) -> GraphQueryResp {
        self.query_thread.request_blocking(request)
    }

    pub fn graph_arc(&self) -> &Arc<PackedGraph> {
        &self.graph
    }

    pub fn graph(&self) -> &PackedGraph {
        &self.graph
    }

    pub fn path_positions_arc(&self) -> &Arc<PathPositionMap> {
        &self.path_positions
    }

    pub fn path_positions(&self) -> &PathPositionMap {
        &self.path_positions
    }

    pub fn build_overlay_colors<F>(&self, mut f: F) -> Vec<rgb::RGB<f32>>
    where
        F: FnMut(&PackedGraph, Handle) -> rgb::RGB<f32>,
    {
        let mut result = Vec::with_capacity(self.graph.node_count());

        let mut handles = self.graph.handles().collect::<Vec<_>>();
        handles.sort();

        for handle in handles {
            let color = f(&self.graph, handle);
            result.push(color);
        }

        result
    }

    pub fn handle_positions(
        &self,
        handle: Handle,
    ) -> Option<Vec<(PathId, StepPtr, usize)>> {
        self.path_positions.handle_positions(&self.graph, handle)
    }
}

struct QueryThread {
    resp_rx: channel::Receiver<GraphQueryResp>,
    req_tx: channel::Sender<GraphQueryRequest>,
    _thread_handle: std::thread::JoinHandle<()>,
}

impl QueryThread {
    fn request_blocking(&self, request: GraphQueryRequest) -> GraphQueryResp {
        self.req_tx.send(request).unwrap();
        self.resp_rx.recv().unwrap()
    }

    fn new(graph: Arc<PackedGraph>) -> Self {
        let (resp_tx, resp_rx) = channel::bounded::<GraphQueryResp>(0);
        let (req_tx, req_rx) = channel::bounded::<GraphQueryRequest>(0);

        let _thread_handle = std::thread::spawn(move || {
            use GraphQueryRequest as Req;
            use GraphQueryResp as Resp;

            use Direction as Dir;

            while let Ok(request) = req_rx.recv() {
                let resp: Resp = match request {
                    Req::GraphStats => Resp::GraphStats {
                        node_count: graph.node_count(),
                        edge_count: graph.edge_count(),
                        path_count: graph.path_count(),
                        total_len: graph.total_length(),
                    },
                    Req::NodeStats(node_id) => {
                        let handle = Handle::pack(node_id, false);

                        let deg_l = graph.degree(handle, Dir::Left);
                        let deg_r = graph.degree(handle, Dir::Right);

                        let coverage: usize = graph
                            .steps_on_handle(handle)
                            .map(|occurs| occurs.count())
                            .unwrap_or(0);

                        Resp::NodeStats {
                            node_id,
                            len: graph.node_len(handle),
                            degree: (deg_l, deg_r),
                            coverage,
                        }
                    }
                    Req::PathStats(path_id) => {
                        let step_count = graph.path_len(path_id).unwrap_or(0);
                        Resp::PathStats {
                            path_id,
                            step_count,
                        }
                    }
                    Req::NodeSeq(node_id) => {
                        let seq =
                            graph.sequence_vec(Handle::pack(node_id, false));
                        let len = seq.len();

                        Resp::NodeSeq { node_id, seq, len }
                    }
                };

                resp_tx.send(resp).unwrap();
            }
        });

        Self {
            resp_rx,
            req_tx,
            _thread_handle,
        }
    }
}

#[derive(Debug, Clone)]
pub enum GraphQueryRequest {
    GraphStats,
    NodeStats(NodeId),
    PathStats(PathId),
    NodeSeq(NodeId),
    // Neighbors(NodeId),
}

#[derive(Debug, Clone)]
pub enum GraphQueryResp {
    GraphStats {
        node_count: usize,
        edge_count: usize,
        path_count: usize,
        total_len: usize,
    },
    NodeStats {
        node_id: NodeId,
        len: usize,
        degree: (usize, usize),
        coverage: usize,
    },
    PathStats {
        path_id: PathId,
        step_count: usize,
    },
    NodeSeq {
        node_id: NodeId,
        seq: Vec<u8>,
        len: usize,
    },
    // Neighbors {
    //     node_id: NodeId,
    //     left: Vec<NodeId>,
    //     right: Vec<NodeId>,
    // },
}
