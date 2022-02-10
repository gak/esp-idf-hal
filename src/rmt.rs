//! Remote Control (RMT) module driver.
//!
//! The RMT (Remote Control) module driver can be used to send and receive infrared remote control
//! signals. Due to flexibility of RMT module, the driver can also be used to generate or receive
//! many other types of signals.
//!
//! This module is an abstraction around the [IDF RMT](https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-reference/peripherals/rmt.html)
//! implementation. It is recommended to read before using this module.
//!
//! This is implementation currently supports transmission only.
//!
//! Not supported:
//! * Interrupts.
//! * Receiving.
//! * Change of config after initialisation.
//!
//! # Example Usage
//!
//! ```
//! // Prepare the config.
//! let config = WriterConfig::new().clock_divider(1);
//!
//! // Retrieve the output pin and channel from peripherals.
//! let peripherals = Peripherals::take().unwrap();
//! let pin = peripherals.pins.gpio18.into_output()?;
//! let channel = peripherals.rmt.channel0;
//!
//! // Create the RMT writer.
//! let writer = Writer::new(pin, channel, &config)?;
//!
//! // Prepare signal pulse signal to be sent.
//! let low = Pulse::new(PinState::Low, PulseTicks::new(10)?);
//! let high = Pulse::new(PinState::High, PulseTicks::new(10)?);
//! let mut signal = StackPairedSignal::<2>::new();
//! signal.set(0, &(low, high))?;
//! signal.set(1, &(high, low))?;
//!
//! // Transmit the signal.
//! writer.start(signal)?;
//!```
//!
//! See the `examples/` folder of this repository for more.
//!
//! # Loading pulses
//! There are two ways of preparing pulse signal. [StackPairedSignal] and [VecSignal]. These
//! implement the [Signal] trait.
//!
//! [StackPairedSignal] lives on the stack and must have the items set in pairs of [Pulse]s. This is
//! due to the internal implementation of RMT, and const generics limitations.
//!
//! [VecSignal] allows you to use the heap and incrementally add pulse items without knowing the size
//! ahead of time.

use crate::gpio::OutputPin;
use crate::units::Hertz;
use chip::HwChannel;
use config::WriterConfig;
use core::convert::TryFrom;
use core::time::Duration;
use esp_idf_sys::*;

pub use chip::*;

// TODO: Docs
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PinState {
    Low,
    High,
}

/// A `Pulse` has a state (high or low) and a duration in ticks.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Pulse {
    pub ticks: PulseTicks,
    pub pin_state: PinState,
}

impl Pulse {
    pub fn new(pin_state: PinState, ticks: PulseTicks) -> Self {
        Pulse { pin_state, ticks }
    }

    /// Create a `Pulse` using a `Duration`.
    ///
    /// We need to know the ticks frequency (`ticks_hz`), which depends on the prepared channel
    /// within a `Writer`. To get the frequency for the `ticks_hz` argument, use
    /// `Writer::counter_clock()`. For example:
    /// ```
    /// # use esp_idf_sys::EspError;
    /// # use esp_idf_hal::gpio::Output;
    /// # use esp_idf_hal::rmt::Channel::Channel0;
    /// # fn example() -> Result<(), EspError> {
    /// # let peripherals = Peripherals::take()?;
    /// # let led: Gpio18<Output> = peripherals.pins.gpio18.into_output()?;
    /// let mut writer = Writer::new(led, cfg)?;
    /// let ticks_hz = writer.counter_clock()?;
    /// let pulse = Pulse::new_with_duration(ticks_hz, PinState::High, Duration::from_nanos(500))?;
    /// // TODO: Update this
    /// # }
    /// ```
    pub fn new_with_duration(
        ticks_hz: Hertz,
        pin_state: PinState,
        duration: Duration,
    ) -> Result<Self, EspError> {
        let ticks = PulseTicks::new_with_duration(ticks_hz, duration)?;
        Ok(Self::new(pin_state, ticks))
    }
}

// TODO: Docs
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PulseTicks(u16);

impl PulseTicks {
    const MAX: u16 = 32767;

    /// PulseTick needs to be unsigned 15 bits: 0-32767 inclusive.
    pub fn new(v: u16) -> Result<Self, EspError> {
        if v > Self::MAX {
            Err(EspError::from(ESP_ERR_INVALID_ARG as i32).unwrap())
        } else {
            Ok(Self(v))
        }
    }

    /// Use the maximum value of 32767.
    pub fn max() -> Self {
        Self(Self::MAX)
    }

    /// Convert a `Duration` into `PulseTicks`.
    ///
    /// See `Pulse::new_with_duration()` for details.
    pub fn new_with_duration(ticks_hz: Hertz, duration: Duration) -> Result<Self, EspError> {
        let ticks = duration
            .as_nanos()
            .checked_mul(u32::from(ticks_hz) as u128)
            .ok_or(EspError::from(EOVERFLOW as i32).unwrap())?
            / 1_000_000_000;
        let ticks = u16::try_from(ticks).map_err(|_| EspError::from(EOVERFLOW as i32).unwrap())?;

        Self::new(ticks)
    }
}

// TODO: Docs
pub mod config {
    use super::PinState;
    use crate::units::{FromValueType, Hertz};
    use esp_idf_sys::{EspError, ESP_ERR_INVALID_ARG};

    // TODO: Docs
    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    pub struct DutyPercent(pub(super) u8);

    impl DutyPercent {
        // TODO: Docs
        pub fn new(v: u8) -> Result<Self, EspError> {
            if v > 100 {
                Err(EspError::from(ESP_ERR_INVALID_ARG as i32).unwrap())
            } else {
                Ok(Self(v))
            }
        }
    }

    // TODO: Docs
    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    pub struct CarrierConfig {
        // TODO: Docs
        pub frequency: Hertz,
        // TODO: Docs
        pub carrier_level: PinState,
        // TODO: Docs
        pub idle_level: PinState,
        // TODO: Docs
        pub duty_percent: DutyPercent,
    }

    impl Default for CarrierConfig {
        // Defaults from https://github.com/espressif/esp-idf/blob/master/components/driver/include/driver/rmt.h#L101
        fn default() -> Self {
            Self {
                frequency: 38.kHz().into(),
                carrier_level: PinState::High,
                idle_level: PinState::Low,
                duty_percent: DutyPercent(33),
            }
        }
    }

    impl CarrierConfig {
        pub fn new() -> Self {
            Default::default()
        }

        pub fn frequency(mut self, hz: Hertz) -> Self {
            self.frequency = hz;
            self
        }

        pub fn carrier_level(mut self, state: PinState) -> Self {
            self.carrier_level = state;
            self
        }

        pub fn idle_level(mut self, state: PinState) -> Self {
            self.idle_level = state;
            self
        }

        pub fn duty_percent(mut self, duty: DutyPercent) -> Self {
            self.duty_percent = duty;
            self
        }
    }

    // TODO: Docs
    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    pub enum Loop {
        None,
        // TODO: Docs say max is 1023
        Count(u32),
    }

    // TODO: Docs
    pub struct WriterConfig {
        // TODO: Docs
        pub clock_divider: u8,
        pub mem_block_num: u8,
        pub carrier: Option<CarrierConfig>,
        // TODO: `loop` is taken. Maybe can change to repeat even though it doesn't match the IDF.
        pub looping: Loop,
        /// Enable and set the signal level on the output if idle.
        pub idle: Option<PinState>,
        pub aware_dfs: bool,
    }

    impl Default for WriterConfig {
        // Defaults from https://github.com/espressif/esp-idf/blob/master/components/driver/include/driver/rmt.h#L101
        fn default() -> Self {
            Self {
                aware_dfs: false,
                mem_block_num: 1,
                clock_divider: 80,
                looping: Loop::None,
                carrier: None,
                idle: Some(PinState::Low),
            }
        }
    }

    impl WriterConfig {
        pub fn new() -> Self {
            Self::default()
        }

        /// Channel can work during APB clock scaling.
        ///
        /// When set, RMT channel will take REF_TICK or XTAL as source clock. The benefit is, RMT
        /// channel can continue work even when APB clock is changing.
        pub fn aware_dfs(mut self, enable: bool) -> Self {
            self.aware_dfs = enable;
            self
        }

        pub fn mem_block_num(mut self, mem_block_num: u8) -> Self {
            self.mem_block_num = mem_block_num;
            self
        }

        pub fn clock_divider(mut self, divider: u8) -> Self {
            self.clock_divider = divider;
            self
        }

        pub fn looping(mut self, looping: Loop) -> Self {
            self.looping = looping;
            self
        }

        pub fn carrier(mut self, carrier: Option<CarrierConfig>) -> Self {
            self.carrier = carrier;
            self
        }

        pub fn idle(mut self, idle: Option<PinState>) -> Self {
            self.idle = idle;
            self
        }
    }
}

// TODO: Docs
pub struct Writer<P: OutputPin, C: HwChannel> {
    pin: P,
    channel: C,
}

impl<P: OutputPin, C: HwChannel> Writer<P, C> {
    // TODO: Docs
    pub fn new(pin: P, channel: C, config: &WriterConfig) -> Result<Self, EspError> {
        let mut flags = 0;
        if config.aware_dfs {
            flags |= RMT_CHANNEL_FLAGS_AWARE_DFS;
        }

        let carrier_en = config.carrier.is_some();
        let carrier = config.carrier.unwrap_or_default();

        use config::Loop;
        let loop_en = config.looping != Loop::None;
        let loop_count = match config.looping {
            Loop::None => 0,
            Loop::Count(c) => c,
        };

        let sys_config = rmt_config_t {
            rmt_mode: rmt_mode_t_RMT_MODE_TX,
            channel: C::channel(),
            gpio_num: pin.pin(),
            clk_div: config.clock_divider,
            mem_block_num: config.mem_block_num,
            flags,
            __bindgen_anon_1: rmt_config_t__bindgen_ty_1 {
                tx_config: rmt_tx_config_t {
                    carrier_en,
                    carrier_freq_hz: carrier.frequency.into(),
                    carrier_level: carrier.carrier_level as u32,
                    carrier_duty_percent: carrier.duty_percent.0,
                    idle_output_en: config.idle.is_some(),
                    idle_level: config.idle.map(|i| i as u32).unwrap_or(0),
                    // Bug? This doesn't seem to work for longer length data on ESP32S2.
                    loop_en,
                    // Bug? When looping is working, it seems to be ignored at least on my ESP32S2.
                    loop_count,
                },
            },
        };

        unsafe {
            esp!(rmt_config(&sys_config))?;
            esp!(rmt_driver_install(C::channel(), 0, 0))?;
        }

        Ok(Self { pin, channel })
    }

    // TODO: Docs
    pub fn counter_clock(&self) -> Result<Hertz, EspError> {
        let mut ticks_hz: u32 = 0;
        esp!(unsafe { rmt_get_counter_clock(C::channel(), &mut ticks_hz) })?;
        Ok(ticks_hz.into())
    }

    /// Start sending the pulses returning immediately. Non blocking.
    ///
    /// `signal` is captured for safety so that the user can't change the data while transmitting.
    pub fn start<S>(&self, signal: S) -> Result<(), EspError>
    where
        S: Signal,
    {
        self.write_items(&signal, false)
    }

    // TODO: Docs
    pub fn start_blocking<S>(&self, signal: &S) -> Result<(), EspError>
    where
        S: Signal,
    {
        self.write_items(signal, true)
    }

    // TODO: Docs
    fn write_items<S>(&self, signal: &S, block: bool) -> Result<(), EspError>
    where
        S: Signal,
    {
        let items = signal.as_slice();
        esp!(unsafe { rmt_write_items(C::channel(), items.as_ptr(), items.len() as i32, block,) })
    }

    // TODO: Docs
    pub fn stop(&self) -> Result<(), EspError> {
        esp!(unsafe { rmt_tx_stop(C::channel()) })
    }

    // TODO: Docs
    pub fn release(self) -> Result<(P, C), EspError> {
        self.stop()?;
        esp!(unsafe { rmt_driver_uninstall(C::channel()) })?;
        Ok((self.pin, self.channel))
    }
}

/// Signal storage for writer in a format ready for the RMT driver.
pub trait Signal {
    fn as_slice(&self) -> &[rmt_item32_t];
}

/// Stack based signal storage for an RMT signal.
///
/// Use this if you know the length of the pulses ahead of time.
///
/// Internally RMT uses pairs of pulses as part of its data structure, so keep things simple
/// in this implementation, you need to `set` a pair of `Pulse`s for each index.
// TODO: Example
#[derive(Clone)]
pub struct StackPairedSignal<const N: usize>([rmt_item32_t; N]);

impl<const N: usize> StackPairedSignal<N> {
    pub fn new() -> Self {
        Self(
            [rmt_item32_t {
                __bindgen_anon_1: rmt_item32_t__bindgen_ty_1 {
                    // Quick way to set all 32 bits to zero, instead of using `__bindgen_anon_1`.
                    val: 0,
                },
            }; N],
        )
    }

    // TODO: Docs
    pub fn set(&mut self, index: usize, pair: &(Pulse, Pulse)) -> Result<(), EspError> {
        let item = self
            .0
            .get_mut(index)
            .ok_or(EspError::from(ERANGE as i32).unwrap())?;

        // SAFETY: We're overriding all 32 bits, so it doesn't matter what was here before.
        let inner = unsafe { &mut item.__bindgen_anon_1.__bindgen_anon_1 };
        inner.set_level0(pair.0.pin_state as u32);
        inner.set_duration0(pair.0.ticks.0 as u32);
        inner.set_level1(pair.1.pin_state as u32);
        inner.set_duration1(pair.1.ticks.0 as u32);

        Ok(())
    }
}

impl<const N: usize> Signal for StackPairedSignal<N> {
    fn as_slice(&self) -> &[rmt_item32_t] {
        &self.0
    }
}

// TODO: impl<const N: usize> From<&[Pulse; N]> for StackWriterSignal<{ (N + 1) / 2 }> {
// Implementing this caused the compiler to crash!

/// `Vec` heap based storage for an RMT signal.
///
/// Use this for when you don't know the final size of your signal data.
#[derive(Clone)]
pub struct VecSignal {
    items: Vec<rmt_item32_t>,

    // Items contain two pulses. Track if we're adding a new pulse to the first one (true) or if
    // we're changing the second one (false).
    next_item_is_new: bool,
}

impl VecSignal {
    pub fn new() -> Self {
        Self {
            items: vec![],
            next_item_is_new: true,
        }
    }

    // TODO: Docs
    pub fn add<I>(&mut self, pulses: I) -> Result<(), EspError>
    where
        I: IntoIterator<Item = Pulse>,
    {
        for pulse in pulses {
            if self.next_item_is_new {
                let mut inner_item = rmt_item32_t__bindgen_ty_1__bindgen_ty_1::default();
                inner_item.set_level0(pulse.pin_state as u32);
                inner_item.set_duration0(pulse.ticks.0 as u32);
                let item = rmt_item32_t {
                    __bindgen_anon_1: rmt_item32_t__bindgen_ty_1 {
                        __bindgen_anon_1: inner_item,
                    },
                };
                self.items.push(item);
            } else {
                // There should be at least one item in the vec.
                let len = self.items.len();
                let item = self.items.get_mut(len - 1).unwrap();

                // SAFETY: This item was previously populated with the same union field.
                let inner = unsafe { &mut item.__bindgen_anon_1.__bindgen_anon_1 };

                inner.set_level1(pulse.pin_state as u32);
                inner.set_duration1(pulse.ticks.0 as u32);
            }

            self.next_item_is_new = !self.next_item_is_new;
        }

        Ok(())
    }

    // TODO: Docs
    pub fn clear(&mut self) {
        self.next_item_is_new = true;
        self.items.clear();
    }
}

impl Signal for VecSignal {
    fn as_slice(&self) -> &[rmt_item32_t] {
        &self.items
    }
}

mod chip {
    use core::marker::PhantomData;
    use esp_idf_sys::*;

    /// RMT peripheral channel.
    pub trait HwChannel {
        fn channel() -> rmt_channel_t;
    }

    macro_rules! impl_channel {
        ($instance:ident: $channel:expr) => {
            pub struct $instance {
                _marker: PhantomData<rmt_channel_t>,
            }

            impl $instance {
                /// # Safety
                ///
                /// It is safe to instantiate this channel exactly one time.
                pub unsafe fn new() -> Self {
                    $instance {
                        _marker: PhantomData,
                    }
                }
            }

            impl HwChannel for $instance {
                fn channel() -> rmt_channel_t {
                    $channel
                }
            }
        };
    }

    // SOC_RMT_CHANNELS_PER_GROUP defines how many channels there are.

    impl_channel!(CHANNEL0: rmt_channel_t_RMT_CHANNEL_0);
    impl_channel!(CHANNEL1: rmt_channel_t_RMT_CHANNEL_1);
    impl_channel!(CHANNEL2: rmt_channel_t_RMT_CHANNEL_2);
    impl_channel!(CHANNEL3: rmt_channel_t_RMT_CHANNEL_3);
    #[cfg(any(esp32, esp32s3))]
    impl_channel!(CHANNEL4: rmt_channel_t_RMT_CHANNEL_4);
    #[cfg(any(esp32, esp32s3))]
    impl_channel!(CHANNEL5: rmt_channel_t_RMT_CHANNEL_5);
    #[cfg(any(esp32, esp32s3))]
    impl_channel!(CHANNEL6: rmt_channel_t_RMT_CHANNEL_6);
    #[cfg(any(esp32, esp32s3))]
    impl_channel!(CHANNEL7: rmt_channel_t_RMT_CHANNEL_7);

    // TODO: Docs
    pub struct Peripheral {
        pub channel0: CHANNEL0,
        pub channel1: CHANNEL1,
        pub channel2: CHANNEL2,
        pub channel3: CHANNEL3,
        #[cfg(any(esp32, esp32s3))]
        pub channel4: CHANNEL4,
        #[cfg(any(esp32, esp32s3))]
        pub channel5: CHANNEL5,
        #[cfg(any(esp32, esp32s3))]
        pub channel6: CHANNEL6,
        #[cfg(any(esp32, esp32s3))]
        pub channel7: CHANNEL7,
    }

    impl Peripheral {
        // TODO: Docs
        pub unsafe fn new() -> Self {
            Self {
                channel0: CHANNEL0::new(),
                channel1: CHANNEL1::new(),
                channel2: CHANNEL2::new(),
                channel3: CHANNEL3::new(),
                #[cfg(any(esp32, esp32s3))]
                channel4: CHANNEL4::new(),
                #[cfg(any(esp32, esp32s3))]
                channel5: CHANNEL5::new(),
                #[cfg(any(esp32, esp32s3))]
                channel6: CHANNEL6::new(),
                #[cfg(any(esp32, esp32s3))]
                channel7: CHANNEL7::new(),
            }
        }
    }
}
