# Valida $env:VJ_XML contra $env:VJ_XSD con System.Xml (XSD 1.0) y escribe el resultado en JSON.
# Lo lanza xsd.rs con -EncodedCommand; las rutas llegan por variables de entorno, nunca en el texto.
$ErrorActionPreference = 'Stop'
$xsd = $env:VJ_XSD; $xml = $env:VJ_XML; $max = [int]$env:VJ_MAX
$XS = 'http://www.w3.org/2001/XMLSchema'
$doc = New-Object System.Xml.XmlDocument
$doc.Load($xsd)
$ns = New-Object System.Xml.XmlNamespaceManager $doc.NameTable
$ns.AddNamespace('xs', $XS)
$relaxed = New-Object System.Collections.ArrayList
# public.xsd 6.3 tiene un xs:group dentro de xs:all (no válido en XSD 1.0): ese xs:all pasa a ser un
# xs:choice repetible, que admite los mismos hijos en cualquier orden.
foreach ($all in @($doc.SelectNodes('//xs:all[xs:group]', $ns))) {
    $choice = $doc.CreateElement('xs', 'choice', $XS)
    $choice.SetAttribute('minOccurs', '0')
    $choice.SetAttribute('maxOccurs', 'unbounded')
    while ($all.FirstChild) { $choice.AppendChild($all.FirstChild) | Out-Null }
    $all.ParentNode.ReplaceChild($choice, $all) | Out-Null
    $null = $relaxed.Add('xs:all con xs:group dentro → xs:choice repetible')
}
# Y algún tipo declara dos veces el mismo atributo (DefinitionObjectID): se deja una sola declaración.
$groups = @{}
foreach ($g in @($doc.SelectNodes('/xs:schema/xs:attributeGroup', $ns))) {
    $groups[$g.GetAttribute('name')] = @($g.SelectNodes('.//xs:attribute', $ns) | ForEach-Object { $_.GetAttribute('name') + $_.GetAttribute('ref') })
}
foreach ($ct in @($doc.SelectNodes('//xs:complexType | //xs:extension | //xs:restriction', $ns))) {
    $seen = @{}
    foreach ($c in @($ct.ChildNodes)) {
        if ($c.LocalName -eq 'attributeGroup') { foreach ($n in $groups[$c.GetAttribute('ref')]) { $seen[$n] = 1 } }
    }
    foreach ($c in @($ct.ChildNodes)) {
        if ($c.LocalName -ne 'attribute') { continue }
        $n = $c.GetAttribute('name') + $c.GetAttribute('ref')
        if ($seen[$n]) { $ct.RemoveChild($c) | Out-Null; $null = $relaxed.Add("atributo $n declarado dos veces en un tipo") } else { $seen[$n] = 1 }
    }
}
$set = New-Object System.Xml.Schema.XmlSchemaSet
$null = $set.Add([System.Xml.Schema.XmlSchema]::Read((New-Object System.Xml.XmlNodeReader $doc), $null))
$set.Compile()

$st = New-Object System.Xml.XmlReaderSettings
$st.ValidationType = [System.Xml.ValidationType]::Schema
$st.Schemas = $set
$st.DtdProcessing = [System.Xml.DtdProcessing]::Ignore
$st.ValidationFlags = $st.ValidationFlags -bor [System.Xml.Schema.XmlSchemaValidationFlags]::ReportValidationWarnings
$issues = New-Object System.Collections.ArrayList
$groupsOut = [ordered]@{}
$counts = @{ error = 0; warning = 0 }
$st.add_ValidationEventHandler({
    param($s, $e)
    $sev = if ($e.Severity -eq [System.Xml.Schema.XmlSeverityType]::Warning) { 'warning' } else { 'error' }
    $counts[$sev]++
    $line = $e.Exception.LineNumber
    if ($issues.Count -lt $max) {
        $null = $issues.Add([ordered]@{ severity = $sev; line = $line; column = $e.Exception.LinePosition; message = $e.Message })
    }
    $key = "$sev|$($e.Message)"
    if ($groupsOut.Contains($key)) { $groupsOut[$key].count++ } else { $groupsOut[$key] = [ordered]@{ severity = $sev; message = $e.Message; count = 1; first_line = $line } }
})
$fatal = $null
$r = [System.Xml.XmlReader]::Create($xml, $st)
try { while ($r.Read()) {} } catch { $ex = $_.Exception; if ($ex.InnerException) { $ex = $ex.InnerException }; $fatal = $ex.Message } finally { $r.Close() }
$out = [ordered]@{
    errors = $counts.error
    warnings = $counts.warning
    fatal = $fatal
    relaxed = @($relaxed | Select-Object -Unique)
    issues = @($issues)
    summary = @($groupsOut.Values)
}
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::Out.Write(($out | ConvertTo-Json -Depth 4 -Compress))
