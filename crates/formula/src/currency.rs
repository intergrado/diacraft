//! The currency codes: 192 entries, an ID and a code of three capital letters.
//!
//! The table is data and nothing here evaluates. The parser asks it which three letters after a number make a currency literal
//! And the evaluator of `CY` (Task 6 of M2) asks it which ID or code is valid and which code an ID stands for, so
//! `CY(1,126)` is `1.0000ERN`.
//!
//! **Where it differs from the reference page.** The rule follows MS-VSDX, and the page differs in one pair: the page
//! has 126 `TPE` and 127 `ERN`, and this table has 126 `ERN` and 127 `TPE`. Only a numeric ID of 126 or 127 can tell them
//! apart; a code finds the same entry in either. To flip it, swap the two entries and the test.
//!
//! **What is not in it.** ID -1 (MS-VSDX's "undefined", which has no code), the IDs 2 to 9 and 51 to 55 (in neither of the spec's
//! tables), and a code in any case but capitals. A literal does not ignore case; a string that does folds it before it
//! asks, with `to_ascii_uppercase`.

/// One entry of the table: an ID and the code that stands for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Currency {
    /// The ID, which a number argument of `CY` gives. ID 0 is `SYS`, the system's currency; ID 1 is the code of three X, which
    /// shows no symbol.
    pub id: i32,
    /// The code: three capital letters, as a literal and a value hold it.
    pub code: &'static str,
}

const fn c(id: i32, code: &'static str) -> Currency {
    Currency { id, code }
}

/// The 192 entries, in ID order: 0, 1, 10 to 50 and 56 to 204. IDs rise and codes are distinct, which the tests hold.
#[rustfmt::skip]
pub static CURRENCIES: [Currency; 192] = [
    c(0, "SYS"), c(1, "XXX"), c(10, "EUR"), c(11, "USD"), c(12, "ATS"),
    c(13, "AUD"), c(14, "BEF"), c(15, "CAD"), c(16, "CHF"), c(17, "CNY"),
    c(18, "DEM"), c(19, "DKK"), c(20, "ESP"), c(21, "FIM"), c(22, "FRF"),
    c(23, "GBP"), c(24, "GRD"), c(25, "HKD"), c(26, "HUF"), c(27, "IDR"),
    c(28, "IEP"), c(29, "ILS"), c(30, "ITL"), c(31, "JPY"), c(32, "KRW"),
    c(33, "LUF"), c(34, "MXN"), c(35, "MYR"), c(36, "NLG"), c(37, "NOK"),
    c(38, "NZD"), c(39, "PHP"), c(40, "PLZ"), c(41, "PTE"), c(42, "ROL"),
    c(43, "RUR"), c(44, "SEK"), c(45, "SGD"), c(46, "THB"), c(47, "TWD"),
    c(48, "XEU"), c(49, "YUN"), c(50, "ZAR"), c(56, "ARS"), c(57, "BMD"),
    c(58, "BOB"), c(59, "BRR"), c(60, "BSD"), c(61, "CLP"), c(62, "COP"),
    c(63, "CRC"), c(64, "CZK"), c(65, "DOP"), c(66, "ECS"), c(67, "EGP"),
    c(68, "HNL"), c(69, "INR"), c(70, "JMD"), c(71, "JOD"), c(72, "KWD"),
    c(73, "MOP"), c(74, "NIO"), c(75, "PAB"), c(76, "PEN"), c(77, "PKR"),
    c(78, "PYG"), c(79, "SAR"), c(80, "SIT"), c(81, "SKK"), c(82, "SVC"),
    c(83, "TRY"), c(84, "TTD"), c(85, "UYU"), c(86, "VEB"), c(87, "VND"),
    c(88, "BRL"), c(89, "PLN"), c(90, "RUB"), c(91, "YUM"), c(92, "BYB"),
    c(93, "UAH"), c(94, "AFA"), c(95, "ALL"), c(96, "DZD"), c(97, "ADP"),
    c(98, "AOA"), c(99, "XCD"), c(100, "AMD"), c(101, "AWG"), c(102, "AZM"),
    c(103, "BHD"), c(104, "BDT"), c(105, "BBD"), c(106, "BYR"), c(107, "BZD"),
    c(108, "XOF"), c(109, "BTN"), c(110, "BAM"), c(111, "BWP"), c(112, "BND"),
    c(113, "BGL"), c(114, "BGN"), c(115, "BIF"), c(116, "KHR"), c(117, "XAF"),
    c(118, "CVE"), c(119, "KYD"), c(120, "KMF"), c(121, "CDF"), c(122, "HRK"),
    c(123, "CUP"), c(124, "CYP"), c(125, "DJF"), c(126, "ERN"), c(127, "TPE"),
    c(128, "EEK"), c(129, "ETB"), c(130, "FKP"), c(131, "FJD"), c(132, "XPF"),
    c(133, "GMD"), c(134, "GEL"), c(135, "GHC"), c(136, "GIP"), c(137, "GTQ"),
    c(138, "GNF"), c(139, "GWP"), c(140, "GYD"), c(141, "HTG"), c(142, "ISK"),
    c(143, "IRR"), c(144, "IQD"), c(145, "KZT"), c(146, "KES"), c(147, "KPW"),
    c(148, "KGS"), c(149, "LAK"), c(150, "LVL"), c(151, "LBP"), c(152, "LSL"),
    c(153, "LRD"), c(154, "LYD"), c(155, "LTL"), c(156, "MKD"), c(157, "MGF"),
    c(158, "MWK"), c(159, "MVR"), c(160, "MTL"), c(161, "MRO"), c(162, "MUR"),
    c(163, "MDL"), c(164, "MNT"), c(165, "MAD"), c(166, "MZM"), c(167, "MMK"),
    c(168, "NAD"), c(169, "NPR"), c(170, "ANG"), c(171, "NGN"), c(172, "OMR"),
    c(173, "PGK"), c(174, "QAR"), c(175, "RWF"), c(176, "SHP"), c(177, "WST"),
    c(178, "STD"), c(179, "SCR"), c(180, "SLL"), c(181, "SBD"), c(182, "SOS"),
    c(183, "LKR"), c(184, "SDD"), c(185, "SRG"), c(186, "SZL"), c(187, "SYP"),
    c(188, "TJR"), c(189, "TJS"), c(190, "TZS"), c(191, "TOP"), c(192, "TND"),
    c(193, "TMM"), c(194, "UGX"), c(195, "AED"), c(196, "UZS"), c(197, "VUV"),
    c(198, "YER"), c(199, "ZMK"), c(200, "ZWD"), c(201, "VEF"), c(202, "MGA"),
    c(203, "RSD"), c(204, "CSD"),
];

/// The entry whose code is exactly `code`: three capital letters, nothing around them. `None` for any other text, `eur` and
/// ` EUR` and `EURO` included (a literal does not ignore case, and a string that does folds it before it asks).
pub fn currency_by_code(code: &str) -> Option<&'static Currency> {
    CURRENCIES.iter().find(|entry| entry.code == code)
}

/// The entry with this ID, or `None` for an ID that is not listed: -1, 2 to 9, 51 to 55 and anything above 204 (makes it
/// `#VALUE!`).
pub fn currency_by_id(id: i32) -> Option<&'static Currency> {
    CURRENCIES.binary_search_by_key(&id, |entry| entry.id).ok().and_then(|at| CURRENCIES.get(at))
}
