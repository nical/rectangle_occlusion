//! A simple occlusion culling algorithm for axis-aligned rectangles.
//!
//! ## Output
//!
//! Occlusion culling results in two lists of rectangles:
//! 
//! - The opaque list should be rendered first. None of its rectangles overlap so order doesn't matter
//!   withing the opaque pass.
//! - The non-opaque list (or alpha list) which should be rendered in back-to-front order after the opaque pass.
//!
//! The output has minimal overdraw (no overdraw at all for opaque items and as little as possible for alpha ones).
//!
//! ## Algorithm overview
//!
//! The occlusion culling algorithm works in front-to-back order, accumulating rectangle in opaque and non-opaque lists.
//! Each time a rectangle is added, it is first tested against existing opaque rectangles and potentially split into visible
//! sub-rectangles, or even discarded completely. The front-to-back order ensures that once a rectangle is added it does not
//! have to be modified again.
//!
//! ## splitting
//!
//! Partially visible rectangles are split into up to 4 visible sub-rectangles by each intersecting occluder.
//!
//! ```ascii
//!  +----------------------+       +----------------------+
//!  | rectangle            |       |                      |
//!  |                      |       |                      |
//!  |  +-----------+       |       |--+-----------+-------|
//!  |  |occluder   |       |  -->  |  |\\\\\\\\\\\|       |
//!  |  +-----------+       |       |--+-----------+-------|
//!  |                      |       |                      |
//!  +----------------------+       +----------------------+
//! ```
//!
//! In the example above the rectangle is split into 4 visible parts with the central occluded part left out.
//!
//! This implementation favors longer horizontal bands instead creating nine-patches to deal with the corners.
//! The advantage is that it produces less rectangles which is good for the performance of the algorithm, however
//! it could cause artifacts if the resulting rectangles are drawn with a non-axis-aligned transformation.
//!
//! ## Performance
//!
//! The cost of the algorithm grows with the number of opaque rectangle as each new rectangle is tested against
//! all previously addec opaque rectangles.
//!
//! Note that opaque rectangles can either be added as opaque or non-opaque. This means a trade-off between
//! overdraw and number of rectangles can be explored to adjust performance: Small opaque rectangles, especially
//! towards the front of the scene, could be added as non-opaque to avoid causing many splits while adding only 
//! a small amount of overdraw.
//!
//! This implementation is intended to be used with a small number of (opaque) items. A similar implementation
//! could use a spatial acceleration structure for opaque rectangles to perform better with a large amount of
//! occluders.
//!

use euclid::default::*;
use euclid::point2;
use smallvec::SmallVec;

/// A visible part of a rectangle after occlusion culling.
#[derive(Debug, PartialEq)]
pub struct Item {
    pub rectangle: Box2D<f32>,
    pub key: u64,
}

/// A builder that applies occlusion culling with rectangles provided in front-to-back order.
///
/// It is faster than `BackToFrontBuilder`.
pub struct FrontToBackBuilder {
    opaque_items: Vec<Item>,
    alpha_items: Vec<Item>,
}

impl FrontToBackBuilder {
    /// Constructor.
    pub fn new() -> Self {
        FrontToBackBuilder {
            opaque_items: Vec::new(),
            alpha_items: Vec::new(),
        }
    }

    /// Pre-allocating constructor.
    pub fn with_capacity(opaque: usize, alpha: usize) -> Self {
        FrontToBackBuilder {
            opaque_items: Vec::with_capacity(opaque),
            alpha_items: Vec::with_capacity(alpha),
        }
    }

    /// Add a rectangle, potentially splitting it and discarding the occluded parts if any.
    ///
    /// Returns true the rectangle is at least partially visible.
    pub fn add(&mut self, rect: &Box2D<f32>, is_opaque: bool, key: u64) -> bool {
        let mut fragments: SmallVec<[Box2D<f32>; 16]> = SmallVec::new();
        fragments.push(*rect);

        for item in &self.opaque_items {
            if fragments.is_empty() {
                break;
            }
            if item.rectangle.intersects(rect) {
                apply_occluder(&item.rectangle, &mut fragments);
            }
        }

        let list = if is_opaque {
            &mut self.opaque_items
        } else {
            &mut self.alpha_items
        };

        for rect in &fragments {
            list.push(Item {
                rectangle: *rect,
                key,
            });
        }

        !fragments.is_empty()
    }

    /// Returns true if the provided rect is at least partially visible, without adding it.
    pub fn test(&self, rect: &Box2D<f32>) -> bool {
        let mut fragments: SmallVec<[Box2D<f32>; 16]> = SmallVec::new();
        fragments.push(*rect);

        for item in &self.opaque_items {
            if item.rectangle.intersects(rect) {
                apply_occluder(&item.rectangle, &mut fragments);
            }
        }

        !fragments.is_empty()
    }

    /// The visible opaque rectangles (front-to-back order).
    pub fn opaque_items(&self) -> &[Item] {
        &self.opaque_items
    }

    /// The visible non-opaque rectangles (front-to-back order).
    pub fn alpha_items(&self) -> &[Item] {
        &self.alpha_items
    }

    /// Resets the builder to its initial state, preserving memory allocations.
    pub fn clear(&mut self) {
        self.opaque_items.clear();
        self.alpha_items.clear();
    }

    pub fn dump_as_svg(&self, output: &mut dyn std::io::Write) -> std::io::Result<()> {
        use svg_fmt::*;

        let mut w: f32 = 0.0;
        let mut h: f32 = 0.0;

        for item in &self.opaque_items {
            w = w.max(item.rectangle.max.x);
            h = h.max(item.rectangle.max.y);
        }
        for item in &self.alpha_items {
            w = w.max(item.rectangle.max.x);
            h = h.max(item.rectangle.max.y);
        }

        writeln!(output, "{}", BeginSvg { w, h } )?;

        // Use random blue-ish colors for opaque items and and random red-ish colors for
        // non-opaque ones. The colors are seeded from the item key.

        for item in &self.opaque_items {
            let i = ((item.key * 37) % 100) as u8;
            let color = rgb(0, i, 150 + i);
            let r = item.rectangle;

            writeln!(
                output,
                r#"    {}"#,
                rectangle(r.min.x, r.min.y, r.size().width, r.size().height).fill(color).stroke(Stroke::Color(black(), 1.0))
            )?;
        }

        for item in &self.alpha_items {
            let i = ((item.key * 37) % 100) as u8;
            let color = rgb(150 + i, i, 0);
            let r = item.rectangle;

            writeln!(
                output,
                r#"    {}"#,
                rectangle(r.min.x, r.min.y, r.size().width, r.size().height)
                    .fill(color)
                    .opacity(0.6)
                    .stroke(Stroke::Color(black(), 1.0))
            )?;
        }

        writeln!(output, "{}", EndSvg)    }
}


// Split out the parts of the rects in the provided vector
fn apply_occluder(occluder: &Box2D<f32>, rects: &mut SmallVec<[Box2D<f32>; 16]>) {
    // Iterate in reverse order so that we can push new rects at the back without
    // visiting them;
    let mut i = rects.len() - 1;
    loop {
        let r = rects[i];

        if r.intersects(occluder) {
            let top = r.min.y < occluder.min.y && r.max.y > occluder.min.y;
            let bottom = r.max.y > occluder.max.y && r.min.y < occluder.max.y;
            let left = r.min.x < occluder.min.x && r.max.x > occluder.min.x;
            let right = r.max.x > occluder.max.x && r.min.x < occluder.max.x;

            if top {
                rects.push(Box2D {
                    min: r.min,
                    max: point2(r.max.x, occluder.min.y),
                });
            }

            if bottom {
                rects.push(Box2D {
                    min: point2(r.min.x, occluder.max.y),
                    max: r.max,
                });
            }

            if left {
                let min_y = r.min.y.max(occluder.min.y);
                let max_y = r.max.y.min(occluder.max.y);
                rects.push(Box2D {
                    min: point2(r.min.x, min_y),
                    max: point2(occluder.min.x, max_y),
                });
            }

            if right {
                let min_y = r.min.y.max(occluder.min.y);
                let max_y = r.max.y.min(occluder.max.y);
                rects.push(Box2D {
                    min: point2(occluder.max.x, min_y),
                    max: point2(r.max.x, max_y),
                });
            }

            // Remove the original rectangle, replacing it with
            // one of the new ones we just added, or popping it
            // if it is the last item.
            if i == rects.len() {
                rects.pop();
            } else {
                rects.swap_remove(i);
            }
        }

        if i == 0 {
            break;
        }

        i -= 1;
    }
}

/// A back-to-front occlusion culling builder provided for convenience.
///
/// This builder internally reconstructs front-to-back order at the cost
/// of some computation overhead and uses FrontToBackBuilder. For maximum
/// speed it is better to use `FrontToBackBuilder` directly instead.
pub struct BackToFrontBuilder {
    commands: Vec<(Box2D<f32>, bool, u64)>,
    opaque_items: Vec<Item>,
    alpha_items: Vec<Item>,
}

impl BackToFrontBuilder {
    /// Constructor.
    pub fn new() -> Self {
        BackToFrontBuilder {
            commands: Vec::new(),
            opaque_items: Vec::new(),
            alpha_items: Vec::new(),
        }
    }

    /// Add a rectangle in back-to-font order.
    ///
    /// Computation is deferred to the `build()` method.
    pub fn add(&mut self, rect: &Box2D<f32>, is_opaque: bool, key: u64) {
        self.commands.push((*rect, is_opaque, key));
    }

    /// Apply the occlusion culling algorithm to the rectangles provided by prior `add`
    /// invocations.
    pub fn build(&mut self) {
        let cap = self.commands.len();
        self.opaque_items.clear();
        self.opaque_items.reserve(cap);
        self.alpha_items.clear();
        self.alpha_items.reserve(cap);

        let mut builder = FrontToBackBuilder {
            opaque_items: std::mem::take(&mut self.opaque_items),
            alpha_items: std::mem::take(&mut self.alpha_items),
        };

        for cmd in self.commands.iter().rev() {
            builder.add(&cmd.0, cmd.1, cmd.2);
        }

        self.opaque_items = builder.opaque_items;
        self.alpha_items = builder.alpha_items;

        // No need to reverse the opaque list because it does not
        // matter for rendering.
        self.alpha_items.reverse();
        self.commands.clear();
    }

    /// The visible opaque rectangles.
    ///
    /// Opaque items are only accessible after `build()`.
    pub fn opaque_items(&self) -> &[Item] {
        &self.opaque_items
    }

    /// The visible non-opaque rectangles in back-to-front order.
    ///
    /// Opaque items are only accessible after `build()`.
    pub fn alpha_items(&self) -> &[Item] {
        &self.alpha_items
    }
}

#[test]
fn basic() {
    let mut builder = FrontToBackBuilder::new();

    builder.add(&Box2D { min: point2(0.0, 0.0), max: point2(100.0, 100.0) }, true, 0);
    builder.add(&Box2D { min: point2(50.0, 50.0), max: point2(150.0, 150.0) }, false, 1);

    assert_eq!(builder.opaque_items(), &[Item { rectangle: Box2D { min: point2(0.0, 0.0), max: point2(100.0, 100.0) }, key: 0 }]);
    assert_eq!(builder.alpha_items(), &[
        Item { rectangle: Box2D { min: point2(100.0, 50.0), max: point2(150.0, 100.0) }, key: 1 },
        Item { rectangle: Box2D { min: point2(50.0, 100.0), max: point2(150.0, 150.0) }, key: 1 },
    ]);
}

#[test]
fn fully_occluded_1() {
    let mut builder = FrontToBackBuilder::new();

    builder.add(&Box2D { min: point2(0.0, 0.0), max: point2(100.0, 100.0) }, true, 0);
    builder.add(&Box2D { min: point2(0.0, 0.0), max: point2(100.0, 100.0) }, false, 1);
    builder.add(&Box2D { min: point2(10.0, 10.0), max: point2(90.0, 90.0) }, false, 2);

    assert!(builder.alpha_items().is_empty());
}

#[test]
fn fully_occluded_2() {
    let mut builder = FrontToBackBuilder::new();

    builder.add(&Box2D { min: point2(0.0, 0.0), max: point2(100.0, 100.0) }, true, 0);
    builder.add(&Box2D { min: point2(100.0, 0.0), max: point2(200.0, 100.0) }, true, 0);
    builder.add(&Box2D { min: point2(0.0, 100.0), max: point2(100.0, 200.0) }, true, 0);
    builder.add(&Box2D { min: point2(100.0, 100.0), max: point2(200.0, 200.0) }, true, 0);

    builder.add(&Box2D { min: point2(0.0, 0.0), max: point2(200.0, 200.0) }, false, 1);
    builder.add(&Box2D { min: point2(10.0, 10.0), max: point2(190.0, 190.0) }, false, 2);

    assert!(builder.alpha_items().is_empty());
}

#[test]
fn foo() {
    let mut builder = FrontToBackBuilder::new();

    builder.add(&Box2D { min: point2(10.0, 60.0), max: point2(300.0, 300.0) }, true, 1);

    builder.add(&Box2D { min: point2(100.0, 100.0), max: point2(350.0, 350.0) }, false, 6);

    builder.add(&Box2D { min: point2(0.0, 50.0), max: point2(600.0, 500.0) }, true, 2);

    builder.add(&Box2D { min: point2(0.0, 0.0), max: point2(200.0, 100.0) }, true, 3);
    builder.add(&Box2D { min: point2(200.0, 0.0), max: point2(400.0, 100.0) }, true, 4);
    builder.add(&Box2D { min: point2(400.0, 0.0), max: point2(600.0, 100.0) }, true, 5);

    println!("opaque: {:#?}", builder.opaque_items);
    println!("alpha: {:#?}", builder.alpha_items);

    builder.dump_as_svg(&mut std::fs::File::create("tmp.svg").expect("!!")).unwrap();
}

