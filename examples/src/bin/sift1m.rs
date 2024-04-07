#![feature(portable_simd)]
use std::simd::f32x4;

use vectune::{Builder as VamanaBuilder, Point as VectunePoint, Graph as VectuneGraph};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use byteorder::{LittleEndian, ReadBytesExt};
use std::fs::File;
use std::io::{self, BufReader};
use indicatif::ProgressBar;

fn read_fvecs(file_path: &str) -> io::Result<Vec<Vec<f32>>> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let mut vectors = Vec::new();

    while let Ok(dim) = reader.read_i32::<LittleEndian>() {
        let mut vec = Vec::with_capacity(dim as usize);
        for _ in 0..dim {
            let val = reader.read_f32::<LittleEndian>()?;
            vec.push(val);
        }
        vectors.push(vec);
    }

    Ok(vectors)
}

fn read_ivecs(file_path: &str) -> io::Result<Vec<Vec<i32>>> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let mut vectors = Vec::new();

    while let Ok(dim) = reader.read_i32::<LittleEndian>() {
        let mut vec = Vec::with_capacity(dim as usize);
        for _ in 0..dim {
            let val = reader.read_i32::<LittleEndian>()?;
            vec.push(val);
        }
        vectors.push(vec);
    }

    Ok(vectors)
}

fn main() {
    let seed: u64 = 11923543545843533243;
    let mut rng = SmallRng::seed_from_u64(seed);

    // Locad test data
    let base_vectors = read_fvecs("./test_data/sift/sift_base.fvecs").unwrap();
    let query_vectors = read_fvecs("./test_data/sift/sift_query.fvecs").unwrap();
    let groundtruth = read_ivecs("test_data/sift/sift_groundtruth.ivecs").unwrap();

    let mut points = Vec::new();
    for vec in base_vectors{
        points.push(Point(vec.to_vec()));
    }

    println!("building vamana...");
    let vamana_builder = VamanaBuilder::default();
    let (nodes, centroid) = vamana_builder.progress(ProgressBar::new(1000)).build(points);

    let mut graph = Graph {
        nodes,
        backlinks: Vec::new(),
        cemetery: Vec::new(),
        centroid,
    };

    // Search in FreshVamana

    let round = 100;
    let mut hit = 0;
    // println!("query_vectors len: {:?}", &query_vectors[0..100]);
    for _ in 0..round {
        let query_i = rng.gen_range(0..query_vectors.len() as usize);
        let query = &query_vectors[query_i];

        let (vamana_results, _s) = vectune::search(&mut graph, &Point(query.to_vec()), 50);
        let top5 = &vamana_results
            .into_iter()
            .map(|(_, i)| i as i32)
            .collect::<Vec<i32>>()[0..5];
        let top5_groundtruth = &groundtruth[query_i][0..5];
        for res in top5 {
            if top5_groundtruth.contains(res) {
                hit += 1;
            }
        }
    }

    println!("5-recall-rate@5: {}", hit as f32 / (5 * round) as f32);
}

struct Graph<P>
where
    P: VectunePoint,
{
    nodes: Vec<(P, Vec<usize>)>,
    backlinks: Vec<Vec<usize>>,
    cemetery: Vec<usize>,
    centroid: usize,
}

impl<P> VectuneGraph<P> for Graph<P>
where
    P: VectunePoint,
{
    fn alloc(&mut self, _point: P) -> usize {
        todo!()
    }

    fn free(&mut self, _id: &usize) {
        todo!()
    }

    fn cemetery(&self) -> Vec<usize> {
        self.cemetery.clone()
    }

    fn clear_cemetery(&mut self) {
        self.cemetery = Vec::new();
    }

    fn backlink(&self, id: &usize) -> Vec<usize> {
        self.backlinks[*id].clone()
    }

    fn get(&self, id: &usize) -> (P, Vec<usize>) {
        let node = &self.nodes[*id];
        node.clone()
    }

    fn size_l(&self) -> usize {
        125
    }

    fn size_r(&self) -> usize {
        70
    }

    fn size_a(&self) -> f32 {
        2.0
    }

    fn start_id(&self) -> usize {
        self.centroid
    }

    fn overwirte_out_edges(&mut self, id: &usize, edges: Vec<usize>) {
        self.nodes[*id].1 = edges;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Point(Vec<f32>);
impl Point {
    fn to_f32_vec(&self) -> Vec<f32> {
        self.0.iter().copied().collect()
    }
    fn from_f32_vec(a: Vec<f32>) -> Self {
        Point(a.into_iter().collect())
    }
}

impl VectunePoint for Point {
    // fn distance(&self, other: &Self) -> f32 {
    //     self.0
    //         .iter()
    //         .zip(other.0.iter())
    //         .map(|(a, b)| {
    //             let c = a - b;
    //             c * c
    //         })
    //         .sum::<f32>()
    //         .sqrt()
    // }

    fn distance(&self, other: &Self) -> f32 {
        assert_eq!(self.0.len(), other.0.len());

        let mut sum = f32x4::splat(0.0);
        let chunks = self.0.chunks_exact(4).zip(other.0.chunks_exact(4));

        for (a_chunk, b_chunk) in chunks {
            let a_simd = f32x4::from_slice(a_chunk);
            let b_simd = f32x4::from_slice(b_chunk);
            let diff = a_simd - b_simd;
            sum += diff * diff;
        }

        // Convert SIMD vector sum to an array and sum its elements
        let simd_sum: f32 = sum.to_array().iter().sum();

        // Handle remaining elements
        let remainder_start = self.0.len() - self.0.len() % 4;
        let remainder_sum: f32 = self.0[remainder_start..]
            .iter()
            .zip(&other.0[remainder_start..])
            .map(|(a, b)| {
                let diff = a - b;
                diff * diff
            })
            .sum();

        // Calculate the total sum and then the square root
        (simd_sum + remainder_sum).sqrt()
    }

    fn dim() -> u32 {
        384
    }

    fn add(&self, other: &Self) -> Self {
        Point::from_f32_vec(self.to_f32_vec().into_iter().zip(other.to_f32_vec().into_iter()).map(|(x, y)| x + y).collect())
    }
    fn div(&self, divisor: &usize) -> Self {
        Point::from_f32_vec(self.to_f32_vec().into_iter().map(|v| v / *divisor as f32).collect())
    }

}
