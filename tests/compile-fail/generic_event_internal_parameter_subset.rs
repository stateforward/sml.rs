use sml::sml;

pub struct Start<T, const N: usize>(T, [u8; N]);
pub struct Finished<T>(T);

sml! {
    InternalSubset<T: Clone, const N: usize> {
        *Idle + event<Start<T, N>> = Ready,
         Ready + completion<Finished>(Finished<T>) / finish = X,
    }
}

fn main() {}
