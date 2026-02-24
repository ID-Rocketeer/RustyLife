use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const TARGET_GENERATIONS: usize = 1000;
const SAMPLES: usize = 5;

// Placeholders for RLE data - populated via separate edits to avoid constraints
const BREEDER1_RLE: &str = r#"
#N Breeder 1
#O Bill Gosper
#C The first pattern to be found that exhibits quadratic growth. Found
#C  in the early 1970s.
#C www.conwaylife.com/wiki/index.php?title=Breeder_1
x = 749, y = 338, rule = b3/s23
404bo2bo341b$408bo340b$404bo3bo340b$405b4o340b$416b2o331b$402bo11bo4bo
329b$400bobo17bo328b$342bobo46bo8bobo11bo5bo328b$342bobo44bo3bo21b6o5b
6o317b$331bo10bob2o48bo30bo5bo317b$329bo3bo10b2o43bo4bo36bo317b$334bo
6bo2bo45b5o30bo4bo318b$329bo4bo7b2o83b2o320b$330b5o50b2o362b$385b2o32b
3o5b2o320b$385b2o2bo13bo13bo3bo4bob2o319b$368b2o12b2ob2o3bo11bobo12bo
7b2obobo318b$355b2o10bo2bo8b2o2bobo4bo4b2o4bo9b2o4bo7bob2ob2o317b$355b
2o11b2o9b2o2b3o3bo5b2o5bo8b2o5bo2bo3bo3b2o318b$419bobo5b3o319b2$419bob
o5b3o319b$355b2o11b2o9b2o2b3o3bo5b2o5bo8b2o5bo2bo3bo3b2o318b$355b2o10b
o2bo8b2o2bobo4bo4b2o4bo9b2o4bo7bob2ob2o317b$368b2o12b2ob2o3bo11bobo12b
o7b2obobo318b$385b2o2bo13bo13bo3bo4bob2o319b$385b2o32b3o5b2o320b$330b
5o50b2o362b$329bo4bo7b2o83b2o320b$334bo6bo2bo45b5o30bo4bo318b$329bo3bo
10b2o43bo4bo36bo317b$331bo10bob2o48bo30bo5bo317b$342bobo44bo3bo21b6o5b
6o317b$342bobo46bo8bobo11bo5bo328b$400b2o18bo328b$401bo12bo4bo329b$
416b2o331b$477b2o270b$475b2ob2o269b$475b4o270b$476b2o271b$376bobo370b$
376b2o111b2o258b$377bo107b4ob2o5b4o248b$485b6o5b6o247b$463b2o21b4o6b4o
b2o246b$460b3ob2o34b2o247b$403b2o55b5o21bo262b$400b3ob2o46bo8b3o23bo
261b$352bobo45b5o21b3o23bo32b4o260b$352b2o47b3o20bo2b2o54b2o3bob2o257b
$353bo69bo3bobo26bo11b2o14bobobobo7b2o249b$421bo3bo9bobo13b4obo10bo2bo
13bo3b3o2bo3bo2b2o247b$405b2o14bo3b2o11bo11bo2bob2o4b2o4bobo7b2o6bobo
3bo4b3o2bo247b$405b2o14bo8bo3b2obo12bobo8b2o5bo8b2o7bo4b3o2bo4bo247b$
422bo3bo3b3o3bo14bo44b5o248b2$328bobo91bo3bo3b3o3bo14bo44b5o248b$328b
2o11b2o30b2o30b2o14bo8bo3b2obo12bobo8b2o5bo8b2o7bo4b3o2bo4bo247b$329bo
11b2o30b2o30b2o14bo3b2o11bo11bo2bob2o4b2o4bobo7b2o6bobo3bo4b3o2bo247b$
421bo3bo9bobo13b4obo10bo2bo13bo3b3o2bo3bo2b2o247b$423bo3bobo26bo11b2o
14bobobobo7b2o249b$424bo2b2o54b2o3bob2o257b$426b3o23bo32b4o260b$452bo
8b3o23bo261b$460b5o21bo262b$460b3ob2o34b2o89b2o156b$463b2o21b4o6b4ob2o
87b4o155b$485b6o5b6o88b2ob2o154b$485b4ob2o5b4o91b2o155b$489b2o258b$
464bo136b4o144b$464bobo133b6o143b$280bobo181b2o134b4ob2o142b$280b2o
294b3o13b3o9b2o9b2o132b$281bo252bobo38b5o12bo18b4ob2o131b$516b3o14b2o
2bo37b3ob2o12bo17b6o132b$515b5o12b3o2bo40b2o27bo4b4o133b$515b3ob2o10b
3o72bo2bo139b$440bo77b2o11bobo2b2o59b3o149b$440bobo87b2ob3obo38b2o17b
5o12b2o135b$440b2o89bo6bo37b2o16b3o14bo3bo133b$532bo4bo20b2o15bo2bo14b
o3bo9bo3bo4bo132b$534b5o6b2o10bo2bo8b2o4bobo7b2o5b2o2bo4b2o5bobo5bo
132b$538b2o5b2o11b2o9b2o5bo8b2o5bo6b3o9b2ob3o132b$538b2o53b8o148b2$
538b2o53b8o148b$449b2o30b2o30b2o23b2o5b2o11b2o9b2o5bo8b2o5bo6b3o9b2ob
3o81b2o49b$232bobo214b2o30b2o30b2o19b5o6b2o10bo2bo8b2o4bobo7b2o5b2o2bo
4b2o5bobo5bo79b2ob2o48b$232b2o298bo4bo20b2o15bo2bo14bo3bo9bo3bo4bo79b
4o49b$233bo297bo6bo37b2o16b3o14bo3bo81b2o50b$530b2ob3obo38b2o17b5o12b
2o135b$531bobo2b2o59b3o110b2o37b$531b3o72bo2bo96b4ob2o5b4o27b$392bo
139b3o2bo40b2o27bo4b4o90b6o5b6o26b$392bobo138b2o2bo37b3ob2o12bo17b6o
67b2o21b4o6b4ob2o25b$392b2o140bobo38b5o12bo18b4ob2o63b3ob2o34b2o26b$
576b3o13b3o9b2o9b2o7b2o55b5o21bo41b$600b4ob2o14b3ob2o46bo8b3o23bo40b$
600b6o15b5o21b3o23bo32b4o39b$601b4o17b3o20bo2b2o54b2o3bob2o36b$644bo3b
obo26bo11b2o14bobobobo7b2o28b$642bo3bo9bobo13b4obo10bo2bo13bo3b3o2bo3b
o2b2o26b$569bo56b2o14bo3b2o11bo11bo2bob2o4b2o4bobo7b2o6bobo3bo4b3o2bo
26b$184bobo381bo57b2o14bo8bo3b2obo12bobo8b2o5bo8b2o7bo4b3o2bo4bo26b$
184b2o382b3o72bo3bo3b3o3bo14bo44b5o27b$185bo563b$643bo3bo3b3o3bo14bo
44b5o27b$562b2o30b2o30b2o14bo8bo3b2obo12bobo8b2o5bo8b2o7bo4b3o2bo4bo
26b$562b2o30b2o30b2o14bo3b2o11bo11bo2bob2o4b2o4bobo7b2o6bobo3bo4b3o2bo
26b$344bo297bo3bo9bobo13b4obo10bo2bo13bo3b3o2bo3bo2b2o26b$344bobo198bo
98bo3bobo26bo11b2o14bobobobo7b2o28b$344b2o198bo100bo2b2o54b2o3bob2o36b
$544b3o100b3o23bo32b4o39b$673bo8b3o23bo40b$681b5o21bo41b$681b3ob2o34b
2o26b$684b2o21b4o6b4ob2o25b$706b6o5b6o26b$706b4ob2o5b4o27b$136bobo571b
2o37b$136b2o547bo63b$137bo547bobo61b$685b2o62b3$296bo452b$296bobo198bo
251b$296b2o198bo252b$496b3o162bo55b2o30b$661bobo52b4o29b$661b2o53b2ob
2o28b$718b2o29b2$727b4o18b$726b6o17b$88bobo635b4ob2o16b$88b2o547bo64b
3o25b2o9b2o6b$89bo547bobo61b5o31b4ob2o5b$637b2o62b3ob2o16b2o12b6o6b$
665b2o37b2o16bobo13b4o7b$664bo2bo53bo2bo12bo11b$248bo412b2obo61bo9bobo
10b$248bobo198bo216b2o52bo15bobo10b$248b2o198bo212bo3b2o39b2o17bobobo
10b2o7b$448b3o212bobo22b2o15bo2bo13bo2bobo2bo9bobo6b$643b2o30b2o10bo2b
o4bo3b2o4bobo7b2o6b2o5bo2bo8bo6b$643b2o23b2o5b2o11b2o5bo3b2o5bo8b2o11b
obobo2bo4b3o6b$668b2o55b2obo3bo2bo13b2$668b2o55b2obo3bo2bo13b$3b2o30b
2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b
2o30b2o30b2o30b2o30b2o30b2o23b2o5b2o11b2o5bo3b2o5bo8b2o11bobobo2bo4b3o
6b$3b2o30b2o3bobo24b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b
2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o10bo2bo4bo3b2o4bobo7b2o
6b2o5bo2bo8bo6b$40b2o547bo73bobo22b2o15bo2bo13bo2bobo2bo9bobo6b$41bo
547bobo69bo3b2o39b2o17bobobo10b2o7b$589b2o75b2o52bo15bobo10b$3bo657b2o
bo61bo9bobo10b$2b3o659bo2bo53bo2bo12bo11b$bo3bo194bo464b2o37b2o16bobo
13b4o7b$ob3obo193bobo198bo299b3ob2o16b2o12b6o6b$b5o194b2o198bo300b5o
31b4ob2o5b$196b2o30b2o30b2o30b2o30b2o30b2o30b2o10b3o17b2o30b2o30b2o30b
2o30b2o152b3o25b2o9b2o6b$35b2o30b2o30b2o30b2o30b2o30bo2bo28bo2bo28bo2b
o28bo2bo28bo2bo28bo2bo28bo2bo28bo2bo28bo2bo28bo2bo28bo2bo175b4o
b2o16b$35bobo29bobo29bobo29bobo29bobo29bo2bo28bo2bo28bo2bo28bo2bo28bo
2bo28bo2bo28bo2bo28bo2bo28bo2bo28bo2bo28bo2bo175b6o17b$36b2o30b
2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b
2o30b2o177b4o18b$6b3o692bo47b$8bo691bo48b$7bo741b$701bobo45b$703bo45b$
701bo47b$702bo46b2$38b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o
30b2o371b4o14b$4b3o30bobo29bobo29bobo29bobo29bobo29bo2bo28bo2bo28bo2bo
28bo2bo28bo2bo28bo2bo369b6o13b$3bo3bo29b2o30b2o30b2o30b2o30b2o30bo2bo
28bo2bo28bo2bo28bo2bo28bo2bo28bo2bo229b2o138b4ob2o12b$2bo5bo189b2o30b
2o30b2o30b2o30b2o30b2o229b2o115b3o25b2o9b2o2b$2b2obob2o193b2o387bo113b
5o15b3o13b4ob2ob$202bobo461b2o37b3ob2o14bo15b6o2b$202bo464b2o39b2o18bo
13b4o3b$5bo659bobo59bo12bo8b$4bobo657b3o71b2o9b$4bobo34b2o621bo2bo62bo
8bo2bo6b$5bo35bobo356b2o262bob3o39b2o19b4o7b3ob2o3b$41bo357b2o264bo2bo
21b2o15bo2bo21bo13bo2b$5b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b
2o30b2o30b2o10bo19b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o19b2o9b2o
10bo2bo8b2o4bobo7b2o5b4o2bob2o12bo2b$5b2o30b2o30b2o30b2o30b2o30b2o30b
2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b2o30b
2o23b2o5b2o11b2o9b2o5bo8b2o5bobobobo4b2o5bo2b2o2b$670b2o53b2o2bobo11b
2o4b2$670b2o53b2o2bobo11b2o4b$422bo30b2o30b2o30b2o30b2o30b2o30b2o30b2o
23b2o5b2o11b2o9b2o5bo8b2o5bobobobo4b2o5bo2b2o2b$422bo30b2o30b2o30b2o
30b2o30b2o30b2o23b2o5b2o19b2o9b2o10bo2bo8b2o4bobo7b2o5b4o2bob2o12bo2b$
424b2o211b2o26bo2bo21b2o15bo2bo21bo13bo2b$250b2o387bo24bob3o39b2o19b4o
7b3ob2o3b$250bobo411bo2bo62bo8bo2bo6b$250bo413b3o71b2o9b$665bobo59bo
12bo8b$667b2o39b2o18bo13b4o3b$89b2o575b2o37b3ob2o14bo15b6o2b$89bobo
356b2o212b2o41b5o15b3o13b4ob2ob$89bo357b2o212b2o43b3o25b2o9b2o2b$449bo
213bo66b4ob2o12b$730b6o13b$731b4o14b2$479b6o6b2o229b2o25b$478bo5bo4bo
4bo225b2ob2o24b$472b2o10bo10bo190b2o32b4o25b$414b2o38b5o12b2o5bo4bo5bo
5bo189b2o34b2o26b$298b2o113b3o37bo4bo14bo6b2o8b6o191bo61b$298bobo93b5o
12b2o3bo41bo27bo262b$298bo94bo4bo12bo3bo37bo3bo28b3o260b$398bo11bo44bo
21bo7bob2o2bo257b$393bo3bo12b3o2b2o59b3o8b3o2bo256b$137b2o256bo14bobob
3o38b2o16b3o10b5o2bo255b$137bobo271b3ob2o20b2o15bo2bo14bo3bo3bo5b3obob
3o215b2o37b$137bo275b2o9b2o10bo2bo8b2o4bobo7b2o5bo2bobo4bo9b2ob2o213b
2o38b$417b2o5b2o11b2o9b2o5bo8b2o5bo8bo9b2ob2o216bo37b$417b2o53b8o252b
6o6b2o3b$731bo5bo4bo4bob$417b2o53b8o257bo10bo$360b2o30b2o23b2o5b2o11b
2o9b2o5bo8b2o5bo8bo9b2ob2o212b5o19bo4bo5bo5bo$360b2o30b2o19b2o9b2o10bo
2bo8b2o4bobo7b2o5bo2bobo4bo9b2ob2o210bo4bo20bobo8b6o$411b3ob2o20b2o15b
o2bo14bo3bo3bo5b3obob3o152b5o59bo37b$410bobob3o38b2o16b3o10b5o2bo152bo
4bo21bo32bo3bo20bo2bo14b$346b2o62b3o2b2o59b3o8b3o2bo158bo20bobo33bo21b
3ob2o13b$346bobo61bo44bo21bo7bob2o2bo154bo3bo24bo53b2o3bob2o11b$346bo
64bo3bo37bo3bo28b3o159bo21bob2o24b2obo12b2o14bo3bo9b3o2b$411b2o3bo41bo
27bo181bob2o25b4ob2o9bo2bo13bobobob2o4bo3b2ob$413b3o37bo4bo14bo6b2o8b
6o155b2o13b3o2b2o7b2ob2o11bo4b2o4b2o4bobo7b2o6b5obobo2bobo2b2o$185b2o
227b2o38b5o12b2o5bo4bo5bo5bo155b2o14b2o2b2o3bo4b3o12bobo8b2o5bo8b2o7bo
4b2o2bo5bob$185bobo284b2o10bo10bo180b2o3b2o14bo40bo3b5o2b$185bo292bo5b
o4bo4bo254b$370b2o107b6o6b2o183b2o3b2o14bo40bo3b5o2b$370bobo278b2o14b
2o2b2o3bo4b3o12bobo8b2o5bo8b2o7bo4b2o2bo5bob$370bo97bo2bo179b2o13b3o2b
2o7b2ob2o11bo4b2o4b2o4bobo7b2o6b5obobo2bobo2b2o$472bo195bob2o25b4ob2o
9bo2bo13bobobob2o4bo3b2ob$468bo3bo175bo21bob2o24b2obo12b2o14bo3bo9b3o
2b$469b4o173bo3bo24bo53b2o3bob2o11b$651bo20bobo33bo21b3ob2o13b$646bo4b
o21bo32bo3bo20bo2bo14b$394b2o251b5o59bo37b$394bobo309bo4bo20bobo8b6o$
394bo312b5o19bo4bo5bo5bo$737bo10bo$731bo5bo4bo4bob$233b2o497b6o6b2o3b$
233bobo513b$233bo487bo2bo24b$418b2o305bo23b$418bobo300bo3bo23b$418bo
303b4o23b6$442b2o305b$442bobo304b$442bo306b3$281b2o466b$281bobo465b$
281bo467b$466b2o281b$466bobo280b$466bo282b$491b2o256b$487b4ob2o5b4o
246b$487b6o5b6o245b$465b2o21b4o6b4ob2o244b$462b3ob2o34b2o245b$462b5o
21bo260b$454bo8b3o23bo259b$428b3o23bo32b4o258b$426bo2b2o54b2o3bob2o
255b$425bo3bobo26bo11b2o14bobobobo7b2o247b$329b2o92bo3bo9bobo13b4obo
10bo2bo13bo3b3o2bo3bo2b2o245b$329bobo11b2o30b2o30b2o14bo3b2o11bo11bo2b
ob2o4b2o4bobo7b2o6bobo3bo4b3o2bo245b$329bo13b2o30b2o30b2o14bo8bo3b2obo
12bobo8b2o5bo8b2o7bo4b3o2bo4bo245b$424bo3bo3b3o3bo14bo44b5o246b2$424bo
3bo3b3o3bo14bo44b5o246b$407b2o14bo8bo3b2obo12bobo8b2o5bo8b2o7bo4b3o2bo
4bo245b$407b2o14bo3b2o11bo11bo2bob2o4b2o4bobo7b2o6bobo3bo4b3o2bo245b$
353b2o68bo3bo9bobo13b4obo10bo2bo13bo3b3o2bo3bo2b2o245b$353bobo69bo3bob
o26bo11b2o14bobobobo7b2o247b$353bo49b3o20bo2b2o54b2o3bob2o255b$402b5o
21b3o23bo32b4o258b$402b3ob2o46bo8b3o23bo259b$405b2o55b5o21bo260b$462b
3ob2o34b2o245b$465b2o21b4o6b4ob2o244b$377b2o108b6o5b6o245b$377bobo107b
4ob2o5b4o246b$377bo113b2o256b2$478b2o269b$477b4o268b$477b2ob2o267b$
479b2o268b$401b2o346b$401bobo16b2o327b$401bo14b4ob2o5b4o317b$416b6o5b
6o316b$374bo19b2o21b4o6b4ob2o315b$372b2obo15b3ob2o34b2o316b$334b2o35bo
3bo15b5o353b$331b3ob2o30b4obobo17b3o354b$331b5o30bobo3b2o12b2o33bo327b
$332b3o20bo4b2o4bobo16bo2bo32b2o5b3o318b$354bobo2b2obo2b2ob2o18bo15bo
14bo2bo3b2o321b$353bo3bo2b2ob3o2b4o11b2o5b2o11b2o12b3o6bo4b2o316b$340b
2o12bo2bo4bo3b2obob2o17b2o4b2o4b3o7b2o4b2o5b2ob2o2bo316b$340b2o12b2ob
2o9b2ob2o11bobo9b2o14b2o5bobo10bo316b$370bo14bo35bo7b3o317b2$370bo14bo
35bo7b3o317b$340b2o12b2ob2o9b2ob2o11bobo9b2o14b2o5bobo10bo316b$340b2o
12bo2bo4bo3b2obob2o17b2o4b2o4b3o7b2o4b2o5b2ob2o2bo316b$353bo3bo2b2ob3o
2b4o11b2o5b2o11b2o12b3o6bo4b2o316b$354bobo2b2obo2b2ob2o18bo15bo14bo2bo
3b2o321b$332b3o20bo4b2o4bobo16bo2bo32b2o5b3o318b$331b5o30bobo3b2o12b2o
33bo327b$331b3ob2o30b4obobo17b3o354b$334b2o35bo3bo15b5o353b$372b2obo
15b3ob2o34b2o316b$374bo19b2o21b4o6b4ob2o315b$416b6o5b6o316b$416b4ob2o
5b4o317b$403b2o15b2o327b$402bo346b$407b2o340b$406b4o339b$406b2ob2o338b
$408b2o!
"#;

const SAWTOOTH_RLE: &str = r#"
#N quadraticsawtooth.rle
#O Alexey Nigin, Martin Grant
#C Constructed by Martin Grant on 7 May 2015, based on a design by
#C Alexey Nigin and input from several other contributors.
#C https://conwaylife.com/forums/viewtopic.php?f=2&t=1650&start=25#p19443
#C https://conwaylife.com/wiki/Quadratic_sawtooth
x = 785, y = 785, rule = B3/S23
188bo$188b3o$191bo$190b2o4$199bo$179b2o5b2o9b2obo$179b2o5b2o8bo4bo$
200bo$183b2o11b4o$183b2o4$206b2o$206b2o2$203b2o5b2o$203b2o5b2o2$219b2o
$219b2o$188bo30b2o$188b3o28bo$191bo26bobo$190b2o11bo14bobo$201bobo15bo
$202b2o2$216b2o3b2o$203b2o11bobobobo$202bobo12b5o$188b2o14bo13b3o$188b
2o29bo$183b2o$183b2o3$180b2o$180b2o39b2o$221bo$184bo37b3o$183b2o39bo$
182b3o$182bo$180bo$180bo3b2o$184b2o$181bo$182bo2bo28bo$182b3o30b2o$
214b2o3$166b2o$165bobo$165bo$164b2o$172b2o$172b2o2$175b2o$175b2o3$172b
2o$172b2o13$244bo$245b2o$244b2o$31b2o3b4o$26b3obo2bo6bo$30bo2bo2b5o$
30bo2bo$32bo$29bobo17b2o$30bo18b2o3$14bo$14bo$14bo$18bo124b2o8bo$14b3o
2bo37b2o84b2o6b3o5bo$13bo4bo38b2o91bo7bobo$13bo3bo117b2o13b2o8bo$14b3o
119bo$136bobo$24bo112b2o$13bobo7b3o$13bobo7bo2bo117b2o$13bobo7b3o39b2o
77b2o12bo8b2o$13bobo7b3o8bobo28b2o91b3o6b2o$14b2o6bob2obo7b2o124bo$23b
o2b3o6bo103bo20b2o13b2o$23b4ob2o108bobo34bo$5b2o3b4o10bob3o110bo33bobo
$3obo2bo6bo11b3o144b2o99bo$4bo2bo2b5o10bob2o26bobo217b2o$4bo2bo17bo2bo
26b3o89b2o17b2o106b2o$6bo18bobo27b2o90b2o11bo5b2o$3bobo20bo132bobo$4bo
156bo$58bo113bo$56bobo112bobo$56b3o113bo$59bo$55b5o$163b2o$163b2o$43b
3o6bo$44b2o3b2obo$43b2o5bobo$3b2o43b3obo$3b2o46b2o7$11b2o$11b2o2$29bob
o$29b3o$29b2o327b3o$304bo53bobo$305b2o37b3ob4o5bo2bo$19b2o11bo271b2o
29b3o5bo2b2obob2obobob3o$19b2o9bobo301bo2b4o5bobo2bo2bobobob2o$30b3o
301bo3b4ob2ob3o3b3obo3b2o$33bo301b2obo3b2o18bo$29b5o300b3o2bobo20bo$
335bo17bo9bo$336bo16bo6bo2$27b2o$27b2o230b2o$259b2o3$262b2o$262b2o2$
259b2o$259b2o$251b2o$252bo$252bobo$253b2o2$122bo$121b4o$120b2ob4o5b2o$
119b3ob2o3bo3bo2bo125bo$120b2ob2o3bo7bo123b2o18b2o19b2o$111b2o8b5o3bo
6bo6b2o117bo18bo19bo32bo$110bobo9bo3b3o7bo6b2o114b3o19bobo7bobo5bobo5b
o27b2o$110bo21bo2bo124bo21b2o6bo2bo5b2o4b3o26b2o$109b2o21b2o145bo9b2o
13bo$277bobo7b2o3bo11b2o$270b2o6b2o9b2o$270b2o18bo2bo$291bobo$267b2o$
93b2o15bo156b2o$93b2o15bo$110bo$270b2o29b3o$83bo20b2o164b2o28bo3bo$82b
obo19bo194bo5bo$70b2o10b2obo8bobo5bobo20bo174bo3bo$70b2o10b2ob2o6bo2bo
5b2o19bobo175b3o$82b2obo6b2o17b2o9bobo11b2o133b3o27b3o$82bobo5b2o3bo
15b3o7bo2bo11b2o135bo32bo2bob2obo2bo$83bo8b2o19b2obo5bobo147bo6b2o5b2o
17b2o2bo4bo2b2o$93bo2bo16bo2bo6bobo153b2o5b2o18bo2bob2obo2bo7bo$94bobo
16b2obo8bo197b3o$104b2o5b3o168b2o38bo$103bobo5b2o149b2o18b2o18bo19b2o$
103bo199bo$102b2o197b3o16bo$319bobo$318bo3bo$319b3o$259b2o2b3o51b2o3b
2o$255b2o4b2o2bo$255b2o3bo4bo$261bo2bo$262bo46bo$307bobo$308b2o$266b2o
$267bo56bo2bo4bo2bo$264b3o55b3o2b6o2b3o$264bo59bo2bo4bo2bo3$340b3o11b
3o$339bo2bo10bo2bo$342bo4b3o6bo5b2o$342bo4bo2bo5bo5bobo$339bobo5bob2o
2bobo8bo$364b2o3$341bo8bo$340b3o5bobo$339b2obo$330bo8b3o4bo2bo$331b2o
7b2o4b3o13b2o$330b2o15bo14bobo$364bo$364b2o3$356b3o$355bo2bo$358bo$
339bo18bo$337bobo15bobo4b2o$338b2o13bo8bobo$353bo10bo22b2o$351b3o10b2o
21b2o$351bo2$384b2o$333b2o49b2o$333b2o$387b2o$362b2o23b2o$330b2o30bobo
30b2o$330b2o32bo30bo$364b2o27bobo$333b2o58b2o$333b2o22bo$358b2o$357b2o
$387bo$387b2o$336bo25b2o22bobo$336b2o24bobo$335bobo26bo$364b2o15bo$
380b2o3bo$380bobo$376bobo2b2o$316b2o5bo52bobo3bobo$315bobo4b3o52bo6b2o
$315bo5b2o2bo$314b2o$361bobo$361b2o5b2o$324b2o25b2o9bo5b2o6b2o$322b3o
25bobo23b2o$323bo9b2o17bo$333b2o36b2o$358bo12b2o$322b2o16b3o14b2o$322b
2o8b3o4bo2bo14bobo8b2o$331bo2bo7bo25b2o7bobo$331b2o9bo33bo4bo$339bobo
34bo4bo$376b2o2$332bob2o40b2o2b2o$332bo2bo5bo34b2o2b2o$332b3o5b3o33b3o
bo$339b2obo34bo$339b3o36bo$340b2o2$319bo65b2o$319bobo63bobo$319b2o66bo
$316b2o69b2o$315bobo38b3o20b2o$315bo39bo2bo20b2o$314b2o42bo$322b2o34bo
17b2o$322b2o31bobo18b2o$308b2o$308b2o15b2o$325b2o52b2o$379b2o$311b2o$
311b2o9b2o$322b2o$308b2o$308b2o$300b2o$301bo$301bobo$302b2o5$309b2o$
308b2o$310bo2$313b3o$312b4o$310b2obo2bo$310b2ob2obo2b2o$311bo7b2o$311b
o5bo$312b2o$315bobo15bo$334b2o$327b2o4b2o$319b2o6b2o$319b2o23b3o$344bo
$324b2o19bo17b2o$324b2o37b2o$337b2o24b2o$327b2o9b2o23b3o$316bobo8b2o8b
o25bobo$316bo2bo43bo2bo$315bo3b2o44b2o$315bo2$315bo2b3o42bo$315b2o45b
4o$317b3o21bo20bo3bo$317b3o20b3o20bo2bo$339b2obo20b3o$339b3o$340b2o$
310b2o$309bobo66bo$309bo69bo$308b2o67bo$316b2o63bo$316b2o38b3o20b2obo$
355bo2bo23bo$319b2o37bo23b2o$319b2o37bo15b2o$355bobo16b2o$323b2o$316b
2o5b2o46b2o$316b2o53b2o2$326b2o$326b2o46b2o$374b2o$323b2o$323b2o$315b
2o$316bo$316bobo$317b2o5$325bo$326bo$326bobo$326bo$327bobo$329bo$327bo
bo$327b2o5b2o$334b2o$347bo$331b2o12bobo$331b2o13b2o3$334b2o$334b2o202b
o$536b3o$535bo$535b2o$368bobo$366bo3bo$323b2o41bo$323b2o33b3o4bo4bo8b
2o$358b3o5bo12b2o113bo34b2o8b2o5b2o$357bo3bo4bo3bo123b3o33b2o7b2o5b2o$
326b2o28bo5bo5bobo126bo31bo$326b2o29bo3bo134b2o23bo3bo16b2o$358b3o160b
2o19b2o$323b2o16bo89bo$323b2o15b3o88b3o88b3o$315b2o22b2obo91bo68b2o2b
2o14bob2o$316bo22b3o46b2o43bo51b2o5b2o8b3o19b2o$316bobo21b2o46b2o43bo
2bo48b2o5b2o15bo$317b2o118bo62bobo19b2o$435bo53b2o10b4obo2bo5b2o4bo$
489b2o11b3ob3o6b2o5b2o$422b2o5b2o$324bo33b2o62b2o5b2o$326bo31b2o$326bo
99b2o84b2o$327b2o97b2o84b2o$326bobo22bo34b2obob2o$327b3o20bobo156b2o5b
2o$329bo12b3o4bob2o33bo5bo116b2o5b2o$327b2o13b3o3b2ob2o10b2o$327b2o5b
2o13bob2o10b2o22b2ob2o50b2o4b2o$334b2o14bobo36bo51bo2bo3bob2o74bo9bo9b
o9bo9bo$351bo14b2o73bo2bo6b4o71b3o7b3o7b3o7b3o7b3o$331b2o7b2o3b2o19b2o
73b2ob2o3b6o63bo10bo9bo9bo9bo9bo66bo$331b2o8b5o40bo56b2o71b2o10b2o8b2o
8b2o8b2o8b2o67b2o$342b3o40bobo129b2o120b2o$343bo40bo3b2o245bo3b6o$334b
2o37b2o9bo3b2o120bo22bo107bo2bo$334b2o37b2o9bo3b2o40b4o56b4o17b2o19b2o
16b4o86b2ob2o$385bobo41bo3bo55bo3bo16b2o20bobo14bo3bo88bo$386bo46bo59b
o59bo14b4o67b3o$429bo2bo56bo2bo56bo2bo14bo3bo$571bo67b3o$546b3o18bo2bo
64b2o2bo$342b2o21b5o144b3o29bo92bobo$342b2o20bob3obo145bo28b2o93b3o$
365bo3bo145bo46b2o3b2o73bo$366b3o191bo6bobo71b2o$367bo191bo2bo6bo69b2o
bo$400b3o155b2o7b3o69bobo$363bo36bo55bo102b2o78b4o$362b2o37bo52bobo
185bo$361b2o4b2o86b2o182bo2bo$351b2o7b3o4b2o269b2obo$351b2o8b2o4b2o12b
2o58b2o58b2o5b4o49b2o5b4o66bo$362b2o15b2ob2o55b2ob2o55b2ob2o3bo3bo47b
2ob2o3bo3bo65bobo$363bo15b4o56b4o56b4o8bo47b4o8bo67b2o$380b2o58b2o9bo
48b2o5bo2bo37b3o9b2o5bo2bo66bob2o$451b2o95bo89b3o$450bobo74b2o20bo25bo
64b2o$528b2o45bo36bo22bob2o2bo$358b2o5b2o29b2o5b2o54bo43b2o22bo46b3o
34b2o23b3o2bo$358b2o5b2o29b2o5b2o53b2o42bobo4b2o2b2o19b2o5b2o68bobo23b
ob2o$458bobo41bo7bo2b2o19b2o5b2o$362b2o36b2o100b3o3bobo48bo14b3o$362b
2o36b2o106b2o27b2o19b2o15bo$537b2o19bobo14bo$368b2o35b3o167bo$366b2ob
3o32bo4bo40b2o5b2o116bo$366b2obobo13b2o17b2obo15b2o25b2o5b2o115b3o$
370bob2o11b2o21b3o12b2o61b2o$411bo41b2o31b2o28bo3bo71b2o$374bo7b2o5b2o
29b2o5b2o24b2o60b2o4bo52b3o15bo$382b2o5b2o29b2o5b2o53b2o5b2o19b2o2b2o
5b2o52bo8bo5bobo$482b2o5b2o19b2o3bo4b2o53bo8bobo3b2o$496bo23bo64bobo$
430b2o13bo2bo7b2o5b2o30bo89bo2bo$430b2o10b2obo2b2o6b2o5b2o30b3o87bobo$
378b2o36b2o24b2obo4bo133bobo7bo$378bo37bo9b2o5b2o7b2o16bobo30b2o26b2o
61bo8b3o$379b3o35b3o6b2o5b2o9bo4b2o13b2o28bo27bo$381bo37bo24b2ob2o11bo
5b2o23b3o25b3o$464bob2o23bo27bo73b3o$461bo2b4o$461bo4bo16b2o108bobo$
437b2o23b4o17b2o108bobo$438bo25b2o3bobo$435b3o32b2o8b2o5b2o89b3o12b3o$
435bo34bo9b2o5b2o89bo$579bo31b2o$593b3o15bo30bo$594bo4bo9bobo29b2o$
598bobo8b2o30bobo$476b2o110b2o6b2o3bo$476bo111bobo5b2o3bo12bo$477b3o
108bo7b2o3bo10b3o$479bo118bobo10bo$599bo11b2o6$606b2o3b2o$609bo$606bo
5bo$607b2ob2o$608bobo$609bo$593b2o14bo$593b2o3$590b2o19b2o$590b2o19bo$
612b3o$593b2o19bo$593b2o13b3o$608bo$609bo$672bo$671b2o$671bobo$595b2o$
594bobo$596bo3b2o5b2o$600b2o5b2o2$604b2o$576b2o26b2o$575bobo$575bo6b3o
$574b2o5bo2bo$584bo5bo36b2o$580bo2bo6bo22bo13b2o$581bobo28bob2o$582bob
o26b2obo9b2o5b2o$584bo27bo11b2o5b2o3$582b2o$582b2o$620b2o$620bo$621b3o
$623bo5$702bo$701b2o$701bobo20$740b2o$741bo$741bobo8b2o$742b2o7bobo$
750b3o4b2ob3o$749b3o4bo2b4o$750b3o4b2o$751bobo$732bo19b2o$731b2o$731bo
bo3$764b2o$764b2o3$767b2o$767b2o2$750b3o11b2o$752bo2b3o6b2o$751bo3bo$
756bo18bo$774bobo$773bo3bo$773bo2bo$773bo2bo$773bobo5$757b2o22b2o$757b
obo21bobo$749b2o8bo23bo$749b2o8b2o22b2o$775b2o$775b2o$733bob2o$732bo2b
2o2b3o5b2o23b2o$732bo6b2o6b2o23b2o$732b2o4b2o$734bo8b2o$736b2o5b2o30b
2o$775b2o3$676bo43b2o$674b3o43b2o$673bo$666bo6b2o41b2o5b2o$665bobo48b
2o5b2o$666bo3$678b2o$670b2o6b2o47b2o$670b2o56bo$725b3o$661b2o62bo$661b
2o2$668b2o6b2o$670bo5bobo6b2o$669bo8bo8bo$678b2o6bo5$687b2o$687bo$685b
obo$685b2o2$670b2o$670b2o2$679b2o$612b2o65b2o6b2o$612b2o73b2o3$675bo$
674bobo$675bo6b2o$595b2o85bo$595b2o15b3o68b3o$611bo3bo69bo2$610bo5bo$
610b2o3b2o3$613bo$612bob2o$612bo$612bo3bo$592b2o3b2o14bo2bo$594b3o16b
5o$593bo3bo15b5o$594bobo15b2o3b2o$595bo17b5o$614b3o$615bo3$592b3o2$
592bobo$591b5o$590b2o3b2o15b2o$590b2o3b2o16bo$602b3o5b3o$610bo4$590b2o
8bo$591bo6b3o$588b3o6bo$588bo8b2o6$592b2o3b2o$595bo$592bo5bo4b2o$593b
2ob2o5b2o$594bobo$595bo$595bo4$597bo$596b3o$595bo3bo$594bob3obo$595b5o
11$597b2o$597b2o4$678b2o$678b2o6$663b2o$663bob3o18b2o$663bobo5b2o13b2o
$663bob2o3b2o$663bo6b3o3$656b5o$656bo$657b3o34b2o$657bobo34b2o$657bo3$
659b2o$658b3o$658bobo3$698b2o$698bobo$698bobo$698bobo$677b2o19bobo$
676b2o$678bo$637b2o58b3o$637bob3o54bo3bo$637bobo5b2o48bo4bo$637bob2o3b
2o48bo2b3o$637bo6b3o28bo19bo$632b2o37b6o22bo$632b2o36bob3ob2o21bo$669b
o3b4o3bo18bo$670b3o2bob3obo$674b2ob3ob2o$675b2ob4o$677bo2$640b2o$640b
2o45bo$686bobo$685bo$684bo2bo$677b5o2bo2bo$672b2o3bo6bo2bob3o$672bobo
3b4o3b2o$648b2o22bobo$648b2o22bobo$672bobo3$671b3o$670bo3bo$669bo4bo$
656b2o10bo2b3o$656b2o11bo$673bo$673bo$673bo!
"#;

struct CompletionTracker {
    current: AtomicUsize,
    limit: u64,
    stop_signal: AtomicBool,
}

impl EngineSubscriber for CompletionTracker {
    fn on_snapshot_available(
        &self,
        generation: u64,
        _data: Arc<Vec<u8>>,
        _telemetry: rustylife_core::Telemetry,
    ) -> bool {
        self.current.fetch_add(1, Ordering::SeqCst);
        if generation >= self.limit {
            self.stop_signal.store(true, Ordering::SeqCst);
        }
        true
    }
}

fn run_single_pass(threads: usize, buckets: usize, rle: &str) -> Duration {
    let space = Arc::new(SimulationSpace::new(buckets));
    space.seed_from_rle(0, 0, rle);

    let tracker = Arc::new(CompletionTracker {
        current: AtomicUsize::new(0),
        limit: TARGET_GENERATIONS as u64,
        stop_signal: AtomicBool::new(false),
    });

    let engine = SimulationEngine::new(space.clone(), threads);
    engine.add_subscriber(tracker.clone());

    let start = Instant::now();
    engine.start();

    // Poll for completion
    while tracker.current.load(Ordering::Relaxed) < TARGET_GENERATIONS {
        std::thread::sleep(Duration::from_millis(10));
        if start.elapsed() > Duration::from_secs(30) {
            println!(
                "TIMEOUT at {}/{}",
                tracker.current.load(Ordering::Relaxed),
                TARGET_GENERATIONS
            );
            break;
        }
    }

    engine.shutdown();
    start.elapsed()
}

fn measure_configuration(name: &str, threads: usize, buckets: usize, rle: &str) -> Duration {
    let mut samples = Vec::with_capacity(SAMPLES);
    print!("| {:<15} | {:<7} |", name, buckets);

    // Warmup
    run_single_pass(threads, buckets, rle);

    for _ in 0..SAMPLES {
        let d = run_single_pass(threads, buckets, rle);
        samples.push(d);
        print!("."); // Progress indicator
    }

    let sum: Duration = samples.iter().sum();
    let mean = sum / (SAMPLES as u32);

    // Calculate StdDev
    let mean_f64 = mean.as_secs_f64();
    let variance: f64 = samples
        .iter()
        .map(|d| {
            let diff = d.as_secs_f64() - mean_f64;
            diff * diff
        })
        .sum::<f64>()
        / (SAMPLES as f64);
    let std_dev = variance.sqrt();

    let gens_per_sec = TARGET_GENERATIONS as f64 / mean.as_secs_f64();

    // Clear progress dots
    print!("\r");
    println!(
        "| {:<15} | {:<7} | {:8.3}s | ±{:.3}s | {:5.1} |",
        name,
        buckets,
        mean.as_secs_f64(),
        std_dev,
        gens_per_sec
    );

    mean
}

fn run_suite(pattern_name: &str, rle: &str) {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8)
        .saturating_sub(1)
        .max(1);

    println!(
        "\n=== Bucket Scaling Benchmark: {} (Threads: {}, Gens: {}, Samples: {}) ===",
        pattern_name, threads, TARGET_GENERATIONS, SAMPLES
    );
    println!("| Configuration   | Buckets | Mean Time | StdDev | Gen/s |");
    println!("|:----------------|:--------|----------:|-------:|------:|");

    let multipliers = [1, 2, 4, 8, 12, 16, 24, 32];

    // Legacy Baseline
    measure_configuration("Legacy", threads, 185, rle);

    // Multipliers
    for &m in &multipliers {
        let buckets = (threads * m) | 1;
        measure_configuration(&format!("Multiplier {}x", m), threads, buckets, rle);
    }
    println!("==========================================================\n");
}

#[test]
#[ignore]
fn bench_breeder1() {
    run_suite("Breeder 1", BREEDER1_RLE);
}

#[test]
#[ignore]
fn bench_sawtooth() {
    run_suite("Quadratic Sawtooth", SAWTOOTH_RLE);
}

#[test]
fn help() {
    println!("\n=== Bucket Sweep Benchmark Usage ===");
    println!("To run the Breeder 1 benchmark:");
    println!("  cargo test --test bucket_sweep bench_breeder1 --release -- --nocapture");
    println!("\nTo run the Quadratic Sawtooth benchmark:");
    println!("  cargo test --test bucket_sweep bench_sawtooth --release -- --nocapture");
    println!("====================================\n");
}
