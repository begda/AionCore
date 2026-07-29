-- Internal Aion CLI agent is hosted in-process. After the CarbonFusion brand
-- rename, update its display name and icon path. Logic fields (agent_type,
-- agent_source, id) are unchanged.
UPDATE agent_metadata
SET name = 'CarbonFusion CLI',
    icon = '/api/assets/logos/brand/carbonfusion.svg'
WHERE id = '632f31d2'
  AND agent_type = 'aionrs'
  AND agent_source = 'internal';