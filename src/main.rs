extern crate byteorder;
extern crate sha1;
extern crate minilzo;
extern crate crc;
extern crate bitbit;
extern crate protobuf;
extern crate hex;
extern crate bl2resources;
extern crate itertools;

mod WillowTwo;

use std::fs::File;
use std::io::{Cursor, Read, Write, Seek, SeekFrom};
use std::str;
use std::collections::btree_map::BTreeMap;
use std::borrow::Borrow;
use std::iter;
use byteorder::{LittleEndian, BigEndian, ByteOrder, ReadBytesExt};
use sha1::Sha1;
use crc::crc32;
use bitbit::{BitReader,MSB,LSB};
use hex::ToHex;
use WillowTwo::{WillowTwoPlayerSaveGame, PackedItemData, PackedWeaponData};
use bl2resources::AssetLibraryManager;
use itertools::Itertools;

/*
fn tell(f: &mut Seek) -> usize {
    f.seek(SeekFrom::Current(0)).unwrap() as usize
}

fn read_varint(rdr: &mut Read) -> u64 {
    let mut value: u64 = 0;
    let mut offset = 0;
    loop {
        let mut buf = [0; 1];
        rdr.read(&mut buf[..]);
        let b = buf[0] as u64;
        value |= (b & 0x7f) << offset;
        if (b & 0x80) == 0 {
            break;
        }
        offset += 7
    }
    value
}

#[derive(Debug)]
enum ProtobufValue {
    Varint(u64),
    Fixed64(u64),
    Fixed32(u32), 
    LengthDelimited(Box<[u8]>),
}

fn read_protobuf_value(rdr: &mut Read, wire_type: usize) -> ProtobufValue {
    match wire_type {
        0 => ProtobufValue::Varint(read_varint(rdr)),
        1 => ProtobufValue::Fixed64(rdr.read_u64::<LittleEndian>().unwrap()),
        5 => ProtobufValue::Fixed32(rdr.read_u32::<LittleEndian>().unwrap()),
        2 => {
            let len = read_varint(rdr) as usize;
            //println!(">>> {}", len);
            let mut buf: Vec<u8> = Vec::with_capacity(len);
            rdr.take(len as u64).read_to_end(&mut buf);
            println!(">>> {}", String::from_utf8(buf.clone()).unwrap());
            ProtobufValue::LengthDelimited(buf.into_boxed_slice())
        },
        _ => panic!("unsupported wire type: {}", wire_type),
    }
}

fn read_protobuf<RS>(rdr: &mut RS, len: usize) -> BTreeMap<usize,Vec<ProtobufValue>> where RS: Read+Seek {
    let mut map: BTreeMap<usize,Vec<ProtobufValue>> = BTreeMap::new();

    while tell(rdr) < len {
        let key = read_varint(rdr) as usize;
        let field_number = key >> 3;
        let wire_type = key & 7;
        let value = read_protobuf_value(rdr, wire_type);

        println!("field number {} wire type {} value {:?}", field_number, wire_type, value);

        let mut v = map.entry(field_number).or_insert_with(|| { vec!() });
        v.push(value);
    }

    map
}
*/

#[derive(Debug)]
enum HuffmanNode {
    Internal(Box<HuffmanNode>, Box<HuffmanNode>),
    Leaf(u8),
}

fn read_huffman_tree<R: Read>(brdr: &mut BitReader<R,MSB>) -> HuffmanNode {
    let is_leaf = brdr.read_bit().unwrap();
    match is_leaf {
        true  => HuffmanNode::Leaf(brdr.read_byte().unwrap()),
        false => {
            let left  = Box::new(read_huffman_tree(brdr));
            let right = Box::new(read_huffman_tree(brdr));
            HuffmanNode::Internal(left, right)
        },
    }
}

fn decompress_huffman<R: Read>(brdr: &mut BitReader<R,MSB>, tree: HuffmanNode, outsize: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(outsize);
    while out.len() < outsize {
        let mut node = &tree;
        while let HuffmanNode::Internal(ref left, ref right) = *node {
            let b = brdr.read_bit().unwrap();
            node = match b {
                false => left,
                true  => right,
            };
        }
        if let HuffmanNode::Leaf(ref c) = *node {
            out.push(*c);
        }
    }
    out
}

fn load_save_file() -> Vec<u8> {
    let mut f = File::open("save0002.sav.new").unwrap();
    let mut buf = vec![];
    f.read_to_end(&mut buf).unwrap();
    buf
}

#[derive(Debug)]
struct AssetReference {
    asset_index:      usize,
    sublibrary_index: usize,
    use_set_id:       bool,
    group:            String,
}

impl AssetReference {
    fn from<R: Read>(alm: &AssetLibraryManager, brdr: &mut BitReader<R,LSB>, group: &str) -> Option<AssetReference> {
        let ref config = alm.configs[group];
        //println!("{:#?}", config);
        let index = brdr.read_bits(config.sublibrary_bits + config.asset_bits).unwrap() as usize;
        if index == config.none_index() {
            return None;
        }
/*
        println!("          index: {:032b}", index);

        let asset_index = index & config.asset_mask();
        println!("     asset_mask: {:032b}", config.asset_mask());
        println!(">>      applied: {:032b}", asset_index);

        let sublibrary_index = (index >> config.asset_bits) & config.sublibrary_mask();
        println!("sublibrary_mask: {:032b}", config.sublibrary_mask());
        println!(">>      applied: {:032b}", sublibrary_index);

        let use_set_id = ((index >> config.asset_bits) & config.use_set_id_mask()) != 0;
        println!("use_set_id_mask: {0:32b}", config.use_set_id_mask());
        println!(">>      applied: {:?}", use_set_id);
*/

        Some(AssetReference {
            asset_index:      index & config.asset_mask(),
            sublibrary_index: (index >> config.asset_bits) & config.sublibrary_mask(),
            use_set_id:       ((index >> config.asset_bits) & config.use_set_id_mask()) != 0,
            group:            group.to_string(),
        })
    }
}

#[derive(Debug)]
struct Weapon {
    typ:                      Option<AssetReference>,
    balance:                  Option<AssetReference>,
    manufacturer:             Option<AssetReference>,
    manufacturer_grade_index: usize,
    game_stage:               usize,
    body_part:                Option<AssetReference>,
    grip_part:                Option<AssetReference>,
    barrel_part:              Option<AssetReference>,
    sight_part:               Option<AssetReference>,
    stock_part:               Option<AssetReference>,
    elemental_part:           Option<AssetReference>,
    accessory_1_part:         Option<AssetReference>,
    accessory_2_part:         Option<AssetReference>,
    material_part:            Option<AssetReference>,
    prefix_part:              Option<AssetReference>,
    title_part:               Option<AssetReference>,
}

fn decode_weapon<R: Read>(alm: &AssetLibraryManager, brdr: &mut BitReader<R,LSB>) -> Weapon {
    Weapon {
        typ:                      AssetReference::from(alm, brdr, "WeaponTypes"),
        balance:                  AssetReference::from(alm, brdr, "BalanceDefs"),
        manufacturer:             AssetReference::from(alm, brdr, "Manufacturers"),
        manufacturer_grade_index: brdr.read_bits(7).unwrap() as usize,
        game_stage:               brdr.read_bits(7).unwrap() as usize,
        body_part:                AssetReference::from(alm, brdr, "WeaponParts"),
        grip_part:                AssetReference::from(alm, brdr, "WeaponParts"),
        barrel_part:              AssetReference::from(alm, brdr, "WeaponParts"),
        sight_part:               AssetReference::from(alm, brdr, "WeaponParts"),
        stock_part:               AssetReference::from(alm, brdr, "WeaponParts"),
        elemental_part:           AssetReference::from(alm, brdr, "WeaponParts"),
        accessory_1_part:         AssetReference::from(alm, brdr, "WeaponParts"),
        accessory_2_part:         AssetReference::from(alm, brdr, "WeaponParts"),
        material_part:            AssetReference::from(alm, brdr, "WeaponParts"),
        prefix_part:              AssetReference::from(alm, brdr, "WeaponParts"),
        title_part:               AssetReference::from(alm, brdr, "WeaponParts"),
    }
}

fn dump_asset_reference(alm: &AssetLibraryManager, set_id: usize, ar: &Option<AssetReference>) {
    match *ar {
        Some(ref ar) => {
            let set = alm.set_by_id(set_id).unwrap();

            let ref asset = set.libraries[&*ar.group].sublibraries[ar.sublibrary_index].assets[ar.asset_index];
            println!("{}", asset);
        },
        None => {},
    }
}

fn dump_weapon(alm: &AssetLibraryManager, set_id: usize, weapon: &Weapon) {
    dump_asset_reference(alm, set_id, &weapon.typ);
    dump_asset_reference(alm, set_id, &weapon.balance);
    dump_asset_reference(alm, set_id, &weapon.manufacturer);
    println!("   manufacturer_grade_index: {}", weapon.manufacturer_grade_index);
    println!("   game_stage: {}", weapon.game_stage);
    dump_asset_reference(alm, set_id, &weapon.body_part);
    dump_asset_reference(alm, set_id, &weapon.grip_part);
    dump_asset_reference(alm, set_id, &weapon.barrel_part);
    dump_asset_reference(alm, set_id, &weapon.sight_part);
    dump_asset_reference(alm, set_id, &weapon.stock_part);
    dump_asset_reference(alm, set_id, &weapon.elemental_part);
    dump_asset_reference(alm, set_id, &weapon.accessory_1_part);
    dump_asset_reference(alm, set_id, &weapon.accessory_2_part);
    dump_asset_reference(alm, set_id, &weapon.material_part);
    dump_asset_reference(alm, set_id, &weapon.prefix_part);
    dump_asset_reference(alm, set_id, &weapon.title_part);
    println!("");
}

/*

    let ref libraries = set.libraries;
    let ref weapon_types_lib = libraries["WeaponTypes"];
    //println!("{:#?}", set);

    let ar = weapon.typ.unwrap();
    println!("{:#?}", ar);

    let ref sublib = weapon_types_lib.sublibraries[ar.sublibrary_index];
    println!("{:#?}", sublib);

    //let ref weapon_type = weapon_types_lib.sublibraries[ar.sublibrary_index].assets[ar.asset_index];
    //println!("WEAPON TYPE: {}", weapon_type);
*/

fn decode_item(item: &PackedWeaponData) {
    let mut data: Vec<u8> = item.get_InventorySerialNumber().to_vec();
    //println!("{:?}", data);
    //println!("{}", data.to_hex());

    let key = BigEndian::read_u32(&data[1..5]);

    bogo_decrypt(&mut data[5..], key);
    let data_len = data.len();

    //println!("{:?}", data);
    println!("{}", data.to_hex());

    let stored_checksum = ((data[5] as u16) << 8) | (data[6] as u16);
    data[5] = 0xff;
    data[6] = 0xff;

    data.extend(iter::repeat(0xff).take(40 as usize - data_len));

    let calc_crc = crc32::checksum_ieee(&data);
    //println!("calc_crc: {:08x}", calc_crc);
    let calc_checksum = (((calc_crc & 0xffff0000) >> 16) ^ (calc_crc & 0xffff)) as u16;

    println!("stored checksum: {:04x}", stored_checksum);
    println!("calc checksum:   {:04x}", calc_checksum);

    assert_eq!(stored_checksum, calc_checksum);

    println!("{}", data.to_hex());

    let mut brdr = BitReader::new(Cursor::new(&*data));

    let is_weapon = brdr.read_bit().unwrap();
    let version = brdr.read_bits(7).unwrap();

    assert!(version >= 3); // XXX no unique_id for lower versions
    let unique_id = brdr.read_bits(32).unwrap(); // XXX BE?

    let check = brdr.read_bits(16).unwrap();
    assert_eq!(check, 0x0000ffff);

    assert!(version >= 2); // XXX no set_id for lower versions
    let set_id = brdr.read_bits(8).unwrap() as usize;

    println!("version:   {}", version);
    println!("is_weapon: {}", is_weapon);
    println!("unique_id: {}", unique_id);
    println!("set_id:    {}", set_id);

    let alm = AssetLibraryManager::data().unwrap();
    let weapon = decode_weapon(&alm, &mut brdr);
    //println!("{:#?}", weapon);

    dump_weapon(&alm, set_id, &weapon);
}

fn bogo_decrypt(data: &mut [u8], key: u32) {
    // no key == not encrypted
    if key == 0 { return; };

    let data_len = data.len();

    let mut xor = ((key as i32) >> 5) as u32;
    for i in 0..data_len {
        xor = (((xor as u64) * 0x10a860c1) % 0xfffffffb) as u32;
        data[i] = data[i] ^ ((xor & 0xff) as u8);
    }

    let swapped = {
        let pivot = data_len - ((key % 32) % (data_len as u32)) as usize;
        let (left, right) = data.split_at_mut(pivot);

        let mut swapped = Vec::with_capacity(left.len()+right.len());
        swapped.extend(right.iter());
        swapped.extend(left.iter());

        swapped
    };

    assert_eq!(data_len, swapped.len());

    for i in 0..data_len {
        data[i] = swapped[i];
    }

    assert_eq!(data_len, data.len());
}

fn main() {
    let mut layer1 = load_save_file();

    let (stored_sha, payload) = layer1.split_at_mut(20);

    let calc_sha = {
        let mut calc_sha = [0; 20];
        let mut sha1 = Sha1::new();
        sha1.update(payload);
        sha1.output(&mut calc_sha);
        calc_sha
    };

    //println!("stored sha: {:?}", stored_sha);
    //println!("calc sha:   {:?}", calc_sha);

    assert_eq!(stored_sha, calc_sha);

    let (decomp_size_buf, compressed) = payload.split_at_mut(4);

    let decomp_size = BigEndian::read_u32(decomp_size_buf.borrow()) as usize;
    //println!("decompressed size: {}", decomp_size);

    let decompressed = minilzo::decompress(&compressed, decomp_size).unwrap();

    let mut rdr = Cursor::new(decompressed); // XXX slices are Read? might not need a cursor then

    let payload_size = rdr.read_u32::<BigEndian>().unwrap() as usize;
    //println!("payload size: {}", payload_size);
    assert_eq!(payload_size, decomp_size-4);

    let mut magic = [0; 3];
    rdr.read(&mut magic[..]);
    //println!("magic: {}", str::from_utf8(&magic).unwrap());
    assert_eq!(magic, [0x57,0x53,0x47]);

    let version = rdr.read_u32::<LittleEndian>().unwrap();
    //println!("version: {}", version);
    assert_eq!(version, 2);

    let stored_crc = rdr.read_u32::<LittleEndian>().unwrap();

    let huffman_decomp_size = rdr.read_u32::<LittleEndian>().unwrap() as usize;

    let mut brdr = BitReader::new(rdr);
    let tree = read_huffman_tree(&mut brdr);

    let huffman_decomp = decompress_huffman(&mut brdr, tree, huffman_decomp_size);

    //println!("huffman expected size: {}", huffman_decomp_size);
    //println!("huffman actual size:   {}", huffman_decomp.len());

    let calc_crc = crc32::checksum_ieee(&huffman_decomp);
    //println!("huffman stored crc: {:x}", stored_crc);
    //println!("huffman calc crc:   {:x}", calc_crc);
    assert_eq!(stored_crc, calc_crc);

    let mut protobuf_rdr = Cursor::new(huffman_decomp);

    //let map = read_protobuf(&mut protobuf_rdr, huffman_decomp_size);
    //println!("{:#?}", map);

    //let mut out = File::create("protobuf").unwrap();
    //out.write(&huffman_decomp);

    let savegame: WillowTwoPlayerSaveGame = protobuf::parse_from_reader(&mut protobuf_rdr).unwrap();
    //println!("{:#?}", savegame);

    //println!("{:?}", huffman_decomp);

/*
    let mut huffman = vec!();
    rdr.read_to_end(&mut huffman).unwrap();
*/

/*
    for item in savegame.get_PackedItemData() {
        decode_item(item);
    }
*/
    let items = savegame.get_PackedWeaponData();
    items.iter().foreach(|ref item| decode_item(item));
}
