/*
 * Created on Wed Aug 23 2023
 *
 * This file is a part of Skytable
 * Skytable (formerly known as TerrabaseDB or Skybase) is a free and open-source
 * NoSQL database written by Sayan Nandan ("the Author") with the
 * vision to provide flexibility in data modelling without compromising
 * on performance, queryability or scalability.
 *
 * Copyright (c) 2023, Sayan Nandan <ohsayan@outlook.com>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 *
*/

use {
    super::GNSEvent,
    crate::{
        engine::{
            core::{
                model::{Field, Model},
                space::Space,
                GlobalNS, {EntityID, EntityIDRef},
            },
            data::uuid::Uuid,
            error::TransactionError,
            error::{RuntimeResult, StorageError},
            idx::{IndexST, IndexSTSeqCns, STIndex, STIndexSeq},
            mem::BufferedScanner,
            ql::lex::Ident,
            storage::v1::inf::{self, map, obj, PersistObject},
        },
        util::EndianQW,
    },
    std::marker::PhantomData,
};

pub struct ModelID<'a>(PhantomData<&'a ()>);
#[derive(Debug, Clone, Copy)]
pub struct ModelIDRef<'a> {
    space_id: super::SpaceIDRef<'a>,
    model_name: &'a str,
    model_uuid: Uuid,
    model_version: u64,
}

impl<'a> ModelIDRef<'a> {
    pub fn new_ref(
        space_name: &'a str,
        space: &'a Space,
        model_name: &'a str,
        model: &'a Model,
    ) -> ModelIDRef<'a> {
        ModelIDRef::new(
            super::SpaceIDRef::new(space_name, space),
            model_name,
            model.get_uuid(),
            model.delta_state().schema_current_version().value_u64(),
        )
    }
    pub fn new(
        space_id: super::SpaceIDRef<'a>,
        model_name: &'a str,
        model_uuid: Uuid,
        model_version: u64,
    ) -> Self {
        Self {
            space_id,
            model_name,
            model_uuid,
            model_version,
        }
    }
}
#[derive(Debug, PartialEq)]
pub struct ModelIDRes {
    space_id: super::SpaceIDRes,
    model_name: Box<str>,
    model_uuid: Uuid,
    model_version: u64,
}

impl ModelIDRes {
    #[cfg(test)]
    pub fn new(
        space_id: super::SpaceIDRes,
        model_name: Box<str>,
        model_uuid: Uuid,
        model_version: u64,
    ) -> Self {
        Self {
            space_id,
            model_name,
            model_uuid,
            model_version,
        }
    }
}
pub struct ModelIDMD {
    space_id: super::SpaceIDMD,
    model_name_l: u64,
    model_version: u64,
    model_uuid: Uuid,
}

impl<'a> PersistObject for ModelID<'a> {
    const METADATA_SIZE: usize =
        sizeof!(u64, 2) + sizeof!(u128) + <super::SpaceID as PersistObject>::METADATA_SIZE;
    type InputType = ModelIDRef<'a>;
    type OutputType = ModelIDRes;
    type Metadata = ModelIDMD;
    fn pretest_can_dec_object(scanner: &BufferedScanner, md: &Self::Metadata) -> bool {
        scanner.has_left(md.model_name_l as usize + md.space_id.space_name_l as usize)
    }
    fn meta_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <super::SpaceID as PersistObject>::meta_enc(buf, data.space_id);
        buf.extend(data.model_name.len().u64_bytes_le());
        buf.extend(data.model_version.to_le_bytes());
        buf.extend(data.model_uuid.to_le_bytes());
    }
    unsafe fn meta_dec(scanner: &mut BufferedScanner) -> RuntimeResult<Self::Metadata> {
        Ok(ModelIDMD {
            space_id: <super::SpaceID as PersistObject>::meta_dec(scanner)?,
            model_name_l: scanner.next_u64_le(),
            model_version: scanner.next_u64_le(),
            model_uuid: Uuid::from_bytes(scanner.next_chunk()),
        })
    }
    fn obj_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <super::SpaceID as PersistObject>::obj_enc(buf, data.space_id);
        buf.extend(data.model_name.as_bytes());
    }
    unsafe fn obj_dec(
        s: &mut BufferedScanner,
        md: Self::Metadata,
    ) -> RuntimeResult<Self::OutputType> {
        Ok(ModelIDRes {
            space_id: <super::SpaceID as PersistObject>::obj_dec(s, md.space_id)?,
            model_name: inf::dec::utils::decode_string(s, md.model_name_l as usize)?
                .into_boxed_str(),
            model_uuid: md.model_uuid,
            model_version: md.model_version,
        })
    }
}

fn with_space<T>(
    gns: &GlobalNS,
    space_id: &super::SpaceIDRes,
    f: impl FnOnce(&Space) -> RuntimeResult<T>,
) -> RuntimeResult<T> {
    let spaces = gns.idx().read();
    let Some(space) = spaces.st_get(&space_id.name) else {
        return Err(TransactionError::OnRestoreDataMissing.into());
    };
    if space.get_uuid() != space_id.uuid {
        return Err(TransactionError::OnRestoreDataConflictMismatch.into());
    }
    f(&space)
}

fn with_space_mut<T>(
    gns: &GlobalNS,
    space_id: &super::SpaceIDRes,
    mut f: impl FnMut(&mut Space) -> RuntimeResult<T>,
) -> RuntimeResult<T> {
    let mut spaces = gns.idx().write();
    let Some(space) = spaces.st_get_mut(&space_id.name) else {
        return Err(TransactionError::OnRestoreDataMissing.into());
    };
    if space.get_uuid() != space_id.uuid {
        return Err(TransactionError::OnRestoreDataConflictMismatch.into());
    }
    f(space)
}

fn with_model_mut<T>(
    gns: &GlobalNS,
    space_id: &super::SpaceIDRes,
    model_id: &ModelIDRes,
    f: impl FnOnce(&mut Model) -> RuntimeResult<T>,
) -> RuntimeResult<T> {
    with_space(gns, space_id, |_| {
        let mut models = gns.idx_models().write();
        let Some(model) = models.get_mut(&EntityIDRef::new(&space_id.name, &model_id.model_name))
        else {
            return Err(TransactionError::OnRestoreDataMissing.into());
        };
        if model.get_uuid() != model_id.model_uuid {
            // this should have been handled by an earlier transaction
            return Err(TransactionError::OnRestoreDataConflictMismatch.into());
        }
        f(model)
    })
}

/*
    create model
*/

#[derive(Debug, Clone, Copy)]
/// The commit payload for a `create model ... (...) with {...}` txn
pub struct CreateModelTxn<'a> {
    space_id: super::SpaceIDRef<'a>,
    model_name: &'a str,
    model: &'a Model,
}

impl<'a> CreateModelTxn<'a> {
    pub const fn new(
        space_id: super::SpaceIDRef<'a>,
        model_name: &'a str,
        model: &'a Model,
    ) -> Self {
        Self {
            space_id,
            model_name,
            model,
        }
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub struct CreateModelTxnRestorePL {
    pub(super) space_id: super::SpaceIDRes,
    pub(super) model_name: Box<str>,
    pub(super) model: Model,
}

pub struct CreateModelTxnMD {
    space_id_meta: super::SpaceIDMD,
    model_name_l: u64,
    model_meta: <obj::ModelLayoutRef<'static> as PersistObject>::Metadata,
}

impl<'a> PersistObject for CreateModelTxn<'a> {
    const METADATA_SIZE: usize = <super::SpaceID as PersistObject>::METADATA_SIZE
        + sizeof!(u64)
        + <obj::ModelLayoutRef<'a> as PersistObject>::METADATA_SIZE;
    type InputType = CreateModelTxn<'a>;
    type OutputType = CreateModelTxnRestorePL;
    type Metadata = CreateModelTxnMD;
    fn pretest_can_dec_object(scanner: &BufferedScanner, md: &Self::Metadata) -> bool {
        scanner.has_left((md.model_meta.p_key_len() + md.model_name_l) as usize)
    }
    fn meta_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        // space ID
        <super::SpaceID as PersistObject>::meta_enc(buf, data.space_id);
        // model name
        buf.extend(data.model_name.len().u64_bytes_le());
        // model meta dump
        <obj::ModelLayoutRef as PersistObject>::meta_enc(buf, obj::ModelLayoutRef::from(data.model))
    }
    unsafe fn meta_dec(scanner: &mut BufferedScanner) -> RuntimeResult<Self::Metadata> {
        let space_id = <super::SpaceID as PersistObject>::meta_dec(scanner)?;
        let model_name_l = scanner.next_u64_le();
        let model_meta = <obj::ModelLayoutRef as PersistObject>::meta_dec(scanner)?;
        Ok(CreateModelTxnMD {
            space_id_meta: space_id,
            model_name_l,
            model_meta,
        })
    }
    fn obj_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        // space id dump
        <super::SpaceID as PersistObject>::obj_enc(buf, data.space_id);
        // model name
        buf.extend(data.model_name.as_bytes());
        // model dump
        <obj::ModelLayoutRef as PersistObject>::obj_enc(buf, obj::ModelLayoutRef::from(data.model))
    }
    unsafe fn obj_dec(
        s: &mut BufferedScanner,
        md: Self::Metadata,
    ) -> RuntimeResult<Self::OutputType> {
        let space_id = <super::SpaceID as PersistObject>::obj_dec(s, md.space_id_meta)?;
        let model_name =
            inf::dec::utils::decode_string(s, md.model_name_l as usize)?.into_boxed_str();
        let model = <obj::ModelLayoutRef as PersistObject>::obj_dec(s, md.model_meta)?;
        Ok(CreateModelTxnRestorePL {
            space_id,
            model_name,
            model,
        })
    }
}

impl<'a> GNSEvent for CreateModelTxn<'a> {
    const OPC: u16 = 3;
    type CommitType = CreateModelTxn<'a>;
    type RestoreType = CreateModelTxnRestorePL;
    fn update_global_state(
        CreateModelTxnRestorePL {
            space_id,
            model_name,
            model,
        }: Self::RestoreType,
        gns: &GlobalNS,
    ) -> RuntimeResult<()> {
        /*
            NOTE(@ohsayan):
            A jump to the second branch is practically impossible and should be caught long before we actually end up
            here (due to mismatched checksums), but might be theoretically possible because the cosmic rays can be wild
            (or well magnetic stuff arounding spinning disks). But we just want to be extra sure. Don't let the aliens (or
            rather, radiation) from the cosmos deter us!
        */
        let mut spaces = gns.idx().write();
        let mut models = gns.idx_models().write();
        let Some(space) = spaces.get_mut(&space_id.name) else {
            return Err(TransactionError::OnRestoreDataMissing.into());
        };
        if space.models().contains(&model_name) {
            return Err(TransactionError::OnRestoreDataConflictAlreadyExists.into());
        }
        if models
            .insert(EntityID::new(&space_id.name, &model_name), model)
            .is_some()
        {
            return Err(TransactionError::OnRestoreDataConflictMismatch.into());
        }
        space.models_mut().insert(model_name);
        Ok(())
    }
}

/*
    alter model add
*/

#[derive(Debug, Clone, Copy)]
/// Transaction commit payload for an `alter model add ...` query
pub struct AlterModelAddTxn<'a> {
    model_id: ModelIDRef<'a>,
    new_fields: &'a IndexSTSeqCns<Box<str>, Field>,
}

impl<'a> AlterModelAddTxn<'a> {
    pub const fn new(
        model_id: ModelIDRef<'a>,
        new_fields: &'a IndexSTSeqCns<Box<str>, Field>,
    ) -> Self {
        Self {
            model_id,
            new_fields,
        }
    }
}
pub struct AlterModelAddTxnMD {
    model_id_meta: ModelIDMD,
    new_field_c: u64,
}
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub struct AlterModelAddTxnRestorePL {
    pub(super) model_id: ModelIDRes,
    pub(super) new_fields: IndexSTSeqCns<Box<str>, Field>,
}
impl<'a> PersistObject for AlterModelAddTxn<'a> {
    const METADATA_SIZE: usize = <ModelID as PersistObject>::METADATA_SIZE + sizeof!(u64);
    type InputType = AlterModelAddTxn<'a>;
    type OutputType = AlterModelAddTxnRestorePL;
    type Metadata = AlterModelAddTxnMD;
    fn pretest_can_dec_object(scanner: &BufferedScanner, md: &Self::Metadata) -> bool {
        scanner.has_left(
            (md.model_id_meta.space_id.space_name_l + md.model_id_meta.model_name_l) as usize,
        )
    }
    fn meta_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <ModelID as PersistObject>::meta_enc(buf, data.model_id);
        buf.extend(data.new_fields.st_len().u64_bytes_le());
    }
    unsafe fn meta_dec(scanner: &mut BufferedScanner) -> RuntimeResult<Self::Metadata> {
        let model_id_meta = <ModelID as PersistObject>::meta_dec(scanner)?;
        let new_field_c = scanner.next_u64_le();
        Ok(AlterModelAddTxnMD {
            model_id_meta,
            new_field_c,
        })
    }
    fn obj_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <ModelID as PersistObject>::obj_enc(buf, data.model_id);
        <map::PersistMapImpl<map::FieldMapSpec<_>> as PersistObject>::obj_enc(buf, data.new_fields);
    }
    unsafe fn obj_dec(
        s: &mut BufferedScanner,
        md: Self::Metadata,
    ) -> RuntimeResult<Self::OutputType> {
        let model_id = <ModelID as PersistObject>::obj_dec(s, md.model_id_meta)?;
        let new_fields = <map::PersistMapImpl<map::FieldMapSpec<IndexSTSeqCns<Box<str>, _>>> as PersistObject>::obj_dec(
            s,
            map::MapIndexSizeMD(md.new_field_c as usize),
        )?;
        Ok(AlterModelAddTxnRestorePL {
            model_id,
            new_fields,
        })
    }
}

impl<'a> GNSEvent for AlterModelAddTxn<'a> {
    const OPC: u16 = 4;
    type CommitType = AlterModelAddTxn<'a>;
    type RestoreType = AlterModelAddTxnRestorePL;
    fn update_global_state(
        AlterModelAddTxnRestorePL {
            model_id,
            new_fields,
        }: Self::RestoreType,
        gns: &GlobalNS,
    ) -> RuntimeResult<()> {
        with_model_mut(gns, &model_id.space_id, &model_id, |model| {
            let mut mutator = model.model_mutator();
            for (field_name, field) in new_fields.stseq_owned_kv() {
                if !mutator.add_field(field_name, field) {
                    return Err(TransactionError::OnRestoreDataConflictMismatch.into());
                }
            }
            Ok(())
        })
    }
}

/*
    alter model remove
*/

#[derive(Debug, Clone, Copy)]
/// Transaction commit payload for an `alter model remove` transaction
pub struct AlterModelRemoveTxn<'a> {
    model_id: ModelIDRef<'a>,
    removed_fields: &'a [Ident<'a>],
}
impl<'a> AlterModelRemoveTxn<'a> {
    pub const fn new(model_id: ModelIDRef<'a>, removed_fields: &'a [Ident<'a>]) -> Self {
        Self {
            model_id,
            removed_fields,
        }
    }
}
pub struct AlterModelRemoveTxnMD {
    model_id_meta: ModelIDMD,
    remove_field_c: u64,
}
#[derive(Debug, PartialEq)]
pub struct AlterModelRemoveTxnRestorePL {
    pub(super) model_id: ModelIDRes,
    pub(super) removed_fields: Box<[Box<str>]>,
}

impl<'a> PersistObject for AlterModelRemoveTxn<'a> {
    const METADATA_SIZE: usize = <ModelID as PersistObject>::METADATA_SIZE + sizeof!(u64);
    type InputType = AlterModelRemoveTxn<'a>;
    type OutputType = AlterModelRemoveTxnRestorePL;
    type Metadata = AlterModelRemoveTxnMD;
    fn pretest_can_dec_object(scanner: &BufferedScanner, md: &Self::Metadata) -> bool {
        scanner.has_left(
            (md.model_id_meta.space_id.space_name_l + md.model_id_meta.model_name_l) as usize,
        )
    }
    fn meta_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <ModelID as PersistObject>::meta_enc(buf, data.model_id);
        buf.extend(data.removed_fields.len().u64_bytes_le());
    }
    unsafe fn meta_dec(scanner: &mut BufferedScanner) -> RuntimeResult<Self::Metadata> {
        let model_id_meta = <ModelID as PersistObject>::meta_dec(scanner)?;
        Ok(AlterModelRemoveTxnMD {
            model_id_meta,
            remove_field_c: scanner.next_u64_le(),
        })
    }
    fn obj_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <ModelID as PersistObject>::obj_enc(buf, data.model_id);
        for field in data.removed_fields {
            buf.extend(field.len().u64_bytes_le());
            buf.extend(field.as_bytes());
        }
    }
    unsafe fn obj_dec(
        s: &mut BufferedScanner,
        md: Self::Metadata,
    ) -> RuntimeResult<Self::OutputType> {
        let model_id = <ModelID as PersistObject>::obj_dec(s, md.model_id_meta)?;
        let mut removed_fields = Vec::with_capacity(md.remove_field_c as usize);
        while !s.eof()
            & (removed_fields.len() as u64 != md.remove_field_c)
            & s.has_left(sizeof!(u64))
        {
            let len = s.next_u64_le() as usize;
            if !s.has_left(len) {
                break;
            }
            removed_fields.push(inf::dec::utils::decode_string(s, len)?.into_boxed_str());
        }
        if removed_fields.len() as u64 != md.remove_field_c {
            return Err(StorageError::InternalDecodeStructureCorruptedPayload.into());
        }
        Ok(AlterModelRemoveTxnRestorePL {
            model_id,
            removed_fields: removed_fields.into_boxed_slice(),
        })
    }
}

impl<'a> GNSEvent for AlterModelRemoveTxn<'a> {
    const OPC: u16 = 5;
    type CommitType = AlterModelRemoveTxn<'a>;
    type RestoreType = AlterModelRemoveTxnRestorePL;
    fn update_global_state(
        AlterModelRemoveTxnRestorePL {
            model_id,
            removed_fields,
        }: Self::RestoreType,
        gns: &GlobalNS,
    ) -> RuntimeResult<()> {
        with_model_mut(gns, &model_id.space_id, &model_id, |model| {
            let mut mutator = model.model_mutator();
            for removed_field in removed_fields.iter() {
                if !mutator.remove_field(&removed_field) {
                    return Err(TransactionError::OnRestoreDataConflictMismatch.into());
                }
            }
            Ok(())
        })
    }
}

/*
    alter model update
*/

#[derive(Debug, Clone, Copy)]
/// Transaction commit payload for an `alter model update ...` query
pub struct AlterModelUpdateTxn<'a> {
    model_id: ModelIDRef<'a>,
    updated_fields: &'a IndexST<Box<str>, Field>,
}

impl<'a> AlterModelUpdateTxn<'a> {
    pub const fn new(
        model_id: ModelIDRef<'a>,
        updated_fields: &'a IndexST<Box<str>, Field>,
    ) -> Self {
        Self {
            model_id,
            updated_fields,
        }
    }
}
pub struct AlterModelUpdateTxnMD {
    model_id_md: ModelIDMD,
    updated_field_c: u64,
}
#[derive(Debug, PartialEq)]
pub struct AlterModelUpdateTxnRestorePL {
    pub(super) model_id: ModelIDRes,
    pub(super) updated_fields: IndexSTSeqCns<Box<str>, Field>,
}

impl<'a> PersistObject for AlterModelUpdateTxn<'a> {
    const METADATA_SIZE: usize = <ModelID as PersistObject>::METADATA_SIZE + sizeof!(u64);
    type InputType = AlterModelUpdateTxn<'a>;
    type OutputType = AlterModelUpdateTxnRestorePL;
    type Metadata = AlterModelUpdateTxnMD;
    fn pretest_can_dec_object(scanner: &BufferedScanner, md: &Self::Metadata) -> bool {
        scanner.has_left(
            md.model_id_md.space_id.space_name_l as usize + md.model_id_md.model_name_l as usize,
        )
    }
    fn meta_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <ModelID as PersistObject>::meta_enc(buf, data.model_id);
        buf.extend(data.updated_fields.st_len().u64_bytes_le());
    }
    unsafe fn meta_dec(scanner: &mut BufferedScanner) -> RuntimeResult<Self::Metadata> {
        let model_id_md = <ModelID as PersistObject>::meta_dec(scanner)?;
        Ok(AlterModelUpdateTxnMD {
            model_id_md,
            updated_field_c: scanner.next_u64_le(),
        })
    }
    fn obj_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <ModelID as PersistObject>::obj_enc(buf, data.model_id);
        <map::PersistMapImpl<map::FieldMapSpec<_>> as PersistObject>::obj_enc(
            buf,
            data.updated_fields,
        );
    }
    unsafe fn obj_dec(
        s: &mut BufferedScanner,
        md: Self::Metadata,
    ) -> RuntimeResult<Self::OutputType> {
        let model_id = <ModelID as PersistObject>::obj_dec(s, md.model_id_md)?;
        let updated_fields =
            <map::PersistMapImpl<map::FieldMapSpec<IndexSTSeqCns<Box<str>, _>>> as PersistObject>::obj_dec(
                s,
                map::MapIndexSizeMD(md.updated_field_c as usize),
            )?;
        Ok(AlterModelUpdateTxnRestorePL {
            model_id,
            updated_fields,
        })
    }
}

impl<'a> GNSEvent for AlterModelUpdateTxn<'a> {
    const OPC: u16 = 6;
    type CommitType = AlterModelUpdateTxn<'a>;
    type RestoreType = AlterModelUpdateTxnRestorePL;
    fn update_global_state(
        AlterModelUpdateTxnRestorePL {
            model_id,
            updated_fields,
        }: Self::RestoreType,
        gns: &GlobalNS,
    ) -> RuntimeResult<()> {
        with_model_mut(gns, &model_id.space_id, &model_id, |model| {
            let mut mutator = model.model_mutator();
            for (field_id, field) in updated_fields.stseq_owned_kv() {
                if !mutator.update_field(&field_id, field) {
                    return Err(TransactionError::OnRestoreDataConflictMismatch.into());
                }
            }
            Ok(())
        })
    }
}

/*
    drop model
*/

#[derive(Debug, Clone, Copy)]
/// Transaction commit payload for a `drop model ...` query
pub struct DropModelTxn<'a> {
    model_id: ModelIDRef<'a>,
}

impl<'a> DropModelTxn<'a> {
    pub const fn new(model_id: ModelIDRef<'a>) -> Self {
        Self { model_id }
    }
}
pub struct DropModelTxnMD {
    model_id_md: ModelIDMD,
}
impl<'a> PersistObject for DropModelTxn<'a> {
    const METADATA_SIZE: usize = <ModelID as PersistObject>::METADATA_SIZE;
    type InputType = DropModelTxn<'a>;
    type OutputType = ModelIDRes;
    type Metadata = DropModelTxnMD;
    fn pretest_can_dec_object(scanner: &BufferedScanner, md: &Self::Metadata) -> bool {
        scanner.has_left(
            md.model_id_md.space_id.space_name_l as usize + md.model_id_md.model_name_l as usize,
        )
    }
    fn meta_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <ModelID as PersistObject>::meta_enc(buf, data.model_id);
    }
    unsafe fn meta_dec(scanner: &mut BufferedScanner) -> RuntimeResult<Self::Metadata> {
        let model_id_md = <ModelID as PersistObject>::meta_dec(scanner)?;
        Ok(DropModelTxnMD { model_id_md })
    }
    fn obj_enc(buf: &mut Vec<u8>, data: Self::InputType) {
        <ModelID as PersistObject>::obj_enc(buf, data.model_id);
    }
    unsafe fn obj_dec(
        s: &mut BufferedScanner,
        md: Self::Metadata,
    ) -> RuntimeResult<Self::OutputType> {
        <ModelID as PersistObject>::obj_dec(s, md.model_id_md)
    }
}

impl<'a> GNSEvent for DropModelTxn<'a> {
    const OPC: u16 = 7;
    type CommitType = DropModelTxn<'a>;
    type RestoreType = ModelIDRes;
    fn update_global_state(
        ModelIDRes {
            space_id,
            model_name,
            model_uuid,
            model_version: _,
        }: Self::RestoreType,
        gns: &GlobalNS,
    ) -> RuntimeResult<()> {
        with_space_mut(gns, &space_id, |space| {
            let mut models = gns.idx_models().write();
            if !space.models_mut().remove(&model_name) {
                return Err(TransactionError::OnRestoreDataMissing.into());
            }
            let Some(removed_model) = models.remove(&EntityIDRef::new(&space_id.name, &model_name))
            else {
                return Err(TransactionError::OnRestoreDataMissing.into());
            };
            if removed_model.get_uuid() != model_uuid {
                return Err(TransactionError::OnRestoreDataConflictMismatch.into());
            }
            Ok(())
        })
    }
}
