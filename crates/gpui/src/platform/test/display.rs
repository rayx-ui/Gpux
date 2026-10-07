use crate::{Bounds, DisplayId, Pixels, PlatformDisplay, Point, px};
use anyhow::{Ok, Result};

#[derive(Debug)]
pub(crate) struct TestDisplay {
    id: DisplayId,
    uuid: uuid::Uuid,
    bounds: Bounds<Pixels>,
}

impl TestDisplay {
    pub fn new() -> Self {
        Self::with_size(crate::size(px(1920.), px(1080.)))
    }

    #[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
    pub fn with_size(size: crate::Size<Pixels>) -> Self {
        TestDisplay {
            id: DisplayId(1),
            uuid: uuid::Uuid::new_v4(),
            bounds: Bounds::new(Point::default(), size),
        }
    }
}

impl PlatformDisplay for TestDisplay {
    fn id(&self) -> crate::DisplayId {
        self.id
    }

    fn uuid(&self) -> Result<uuid::Uuid> {
        Ok(self.uuid)
    }

    fn bounds(&self) -> crate::Bounds<crate::Pixels> {
        self.bounds
    }
}
