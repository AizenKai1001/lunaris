//! The spider's total mass and weight, to put the silk's strongest pull beside them.
use lunaris_creature::Model;
fn main() {
    let m = Model::spider();
    let mass: f32 = m.inv_mass.iter().filter(|v| **v > 0.0).map(|v| 1.0 / v).sum();
    println!("total mass {mass:.4} kg, weight {:.3} N, silk cap 150 N = {:.1} times the weight", mass * 9.81, 150.0 / (mass * 9.81));
}
